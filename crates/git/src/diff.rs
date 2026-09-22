use git2::{Delta, Diff, Repository, Tree};
use serde::Serialize;
use similar::{ChangeTag, TextDiff};
use std::path::Path;

#[derive(Serialize)]
pub struct Segment {
    pub text: String,
    pub emphasized: bool,
}

#[derive(Serialize)]
pub struct DiffLine {
    /// "equal" | "delete" | "insert" | "gap" (a collapsed run of unchanged lines)
    pub tag: &'static str,
    pub old_lineno: Option<usize>,
    pub new_lineno: Option<usize>,
    pub segments: Vec<Segment>,
    /// only set when tag == "gap"
    pub skipped: Option<usize>,
}

#[derive(Serialize)]
pub struct FileDiff {
    pub path: String,
    pub status: String,
    pub additions: usize,
    pub deletions: usize,
    /// "committed" (merge-base..HEAD), "staged" (HEAD..index) or "unstaged" (index..workdir)
    pub section: &'static str,
    pub binary: bool,
    pub lines: Vec<DiffLine>,
}

#[derive(Serialize)]
pub struct DiffResult {
    pub merge_base_oid: String,
    pub head_oid: String,
    pub files: Vec<FileDiff>,
}

fn status_label(status: Delta) -> &'static str {
    match status {
        Delta::Added => "added",
        Delta::Deleted => "deleted",
        Delta::Modified => "modified",
        Delta::Renamed => "renamed",
        Delta::Copied => "copied",
        Delta::Untracked => "added",
        _ => "other",
    }
}

fn changed_paths(diff: &Diff) -> Vec<(String, Delta)> {
    diff.deltas()
        .map(|d| {
            let path = d
                .new_file()
                .path()
                .or_else(|| d.old_file().path())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            (path, d.status())
        })
        .collect()
}

fn blob_bytes_at(tree: &Tree, repo: &Repository, path: &str) -> Option<Vec<u8>> {
    let entry = tree.get_path(Path::new(path)).ok()?;
    let blob = entry.to_object(repo).ok()?.into_blob().ok()?;
    Some(blob.content().to_vec())
}

fn blob_bytes_in_index(index: &git2::Index, repo: &Repository, path: &str) -> Option<Vec<u8>> {
    let entry = index.get_path(Path::new(path), 0)?;
    let blob = repo.find_blob(entry.id).ok()?;
    Some(blob.content().to_vec())
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

/// Line-level diff with word-level emphasis inside changed lines, grouped into
/// hunks with a `context` line window (matching `git diff`'s default of 3).
fn line_diff(old: &str, new: &str) -> (usize, usize, Vec<DiffLine>) {
    let text_diff = TextDiff::from_lines(old, new);
    let mut additions = 0;
    let mut deletions = 0;
    let mut lines = Vec::new();

    for (group_idx, group) in text_diff.grouped_ops(3).iter().enumerate() {
        if group_idx > 0 {
            lines.push(DiffLine {
                tag: "gap",
                old_lineno: None,
                new_lineno: None,
                segments: Vec::new(),
                skipped: Some(0),
            });
        }
        for op in group {
            for change in text_diff.iter_inline_changes(op) {
                let tag = match change.tag() {
                    ChangeTag::Equal => "equal",
                    ChangeTag::Delete => {
                        deletions += 1;
                        "delete"
                    }
                    ChangeTag::Insert => {
                        additions += 1;
                        "insert"
                    }
                };
                let segments = change
                    .iter_strings_lossy()
                    .map(|(emphasized, text)| Segment {
                        text: text.replace('\n', ""),
                        emphasized,
                    })
                    .filter(|s| !s.text.is_empty())
                    .collect();
                lines.push(DiffLine {
                    tag,
                    old_lineno: change.old_index().map(|i| i + 1),
                    new_lineno: change.new_index().map(|i| i + 1),
                    segments,
                    skipped: None,
                });
            }
        }
    }
    (additions, deletions, lines)
}

fn build_file_diff(
    path: String,
    status: Delta,
    section: &'static str,
    old_bytes: Vec<u8>,
    new_bytes: Vec<u8>,
) -> FileDiff {
    if is_binary(&old_bytes) || is_binary(&new_bytes) {
        return FileDiff {
            path,
            status: status_label(status).to_string(),
            additions: 0,
            deletions: 0,
            section,
            binary: true,
            lines: Vec::new(),
        };
    }
    let old_text = String::from_utf8_lossy(&old_bytes);
    let new_text = String::from_utf8_lossy(&new_bytes);
    let (additions, deletions, lines) = line_diff(&old_text, &new_text);
    FileDiff {
        path,
        status: status_label(status).to_string(),
        additions,
        deletions,
        section,
        binary: false,
        lines,
    }
}

fn collect_committed(
    diff: &Diff,
    repo: &Repository,
    old_tree: &Tree,
    new_tree: &Tree,
) -> Vec<FileDiff> {
    changed_paths(diff)
        .into_iter()
        .map(|(path, status)| {
            let old_bytes = blob_bytes_at(old_tree, repo, &path).unwrap_or_default();
            let new_bytes = blob_bytes_at(new_tree, repo, &path).unwrap_or_default();
            build_file_diff(path, status, "committed", old_bytes, new_bytes)
        })
        .collect()
}

fn collect_staged(diff: &Diff, repo: &Repository, head_tree: &Tree, index: &git2::Index) -> Vec<FileDiff> {
    changed_paths(diff)
        .into_iter()
        .map(|(path, status)| {
            let old_bytes = blob_bytes_at(head_tree, repo, &path).unwrap_or_default();
            let new_bytes = blob_bytes_in_index(index, repo, &path).unwrap_or_default();
            build_file_diff(path, status, "staged", old_bytes, new_bytes)
        })
        .collect()
}

fn collect_unstaged(diff: &Diff, repo: &Repository, index: &git2::Index, worktree_path: &str) -> Vec<FileDiff> {
    changed_paths(diff)
        .into_iter()
        .map(|(path, status)| {
            let old_bytes = blob_bytes_in_index(index, repo, &path).unwrap_or_default();
            let new_bytes = std::fs::read(Path::new(worktree_path).join(&path)).unwrap_or_default();
            build_file_diff(path, status, "unstaged", old_bytes, new_bytes)
        })
        .collect()
}

/// Diffs a worktree's HEAD against the merge-base with `base_branch` (committed changes,
/// equivalent to `git diff base_branch...HEAD`), plus HEAD against the index (staged) and the
/// index against the working directory (unstaged). Each file's content is diffed line-by-line
/// with word-level emphasis inside changed lines, so the UI can render a side-by-side view.
pub fn diff_against_base(
    worktree_path: String,
    base_branch: String,
) -> Result<DiffResult, String> {
    let repo = Repository::open(&worktree_path).map_err(|e| e.to_string())?;
    let index = repo.index().map_err(|e| e.to_string())?;

    let head_commit = repo
        .head()
        .and_then(|h| h.peel_to_commit())
        .map_err(|e| e.to_string())?;
    let base_commit = repo
        .find_branch(&base_branch, git2::BranchType::Local)
        .and_then(|b| b.get().peel_to_commit())
        .map_err(|e| e.to_string())?;
    let merge_base_oid = repo
        .merge_base(head_commit.id(), base_commit.id())
        .map_err(|e| e.to_string())?;
    let merge_base_commit = repo.find_commit(merge_base_oid).map_err(|e| e.to_string())?;

    let merge_base_tree = merge_base_commit.tree().map_err(|e| e.to_string())?;
    let head_tree = head_commit.tree().map_err(|e| e.to_string())?;

    let committed_diff = repo
        .diff_tree_to_tree(Some(&merge_base_tree), Some(&head_tree), None)
        .map_err(|e| e.to_string())?;
    let staged_diff = repo
        .diff_tree_to_index(Some(&head_tree), Some(&index), None)
        .map_err(|e| e.to_string())?;
    let mut unstaged_opts = git2::DiffOptions::new();
    unstaged_opts.include_untracked(true).recurse_untracked_dirs(true);
    let unstaged_diff = repo
        .diff_index_to_workdir(Some(&index), Some(&mut unstaged_opts))
        .map_err(|e| e.to_string())?;

    let mut files = collect_committed(&committed_diff, &repo, &merge_base_tree, &head_tree);
    files.extend(collect_staged(&staged_diff, &repo, &head_tree, &index));
    files.extend(collect_unstaged(&unstaged_diff, &repo, &index, &worktree_path));

    Ok(DiffResult {
        merge_base_oid: merge_base_oid.to_string(),
        head_oid: head_commit.id().to_string(),
        files,
    })
}

/// Adds `rel_path` to the index (or removes it, if it was deleted from the working directory).
pub fn stage_path(worktree_path: String, rel_path: String) -> Result<(), String> {
    let repo = Repository::open(&worktree_path).map_err(|e| e.to_string())?;
    let mut index = repo.index().map_err(|e| e.to_string())?;
    if Path::new(&worktree_path).join(&rel_path).exists() {
        index.add_path(Path::new(&rel_path)).map_err(|e| e.to_string())?;
    } else {
        index.remove_path(Path::new(&rel_path)).map_err(|e| e.to_string())?;
    }
    index.write().map_err(|e| e.to_string())
}

/// Resets `rel_path` in the index back to HEAD, undoing a stage (whether it was modified, added, or deleted).
pub fn unstage_path(worktree_path: String, rel_path: String) -> Result<(), String> {
    let repo = Repository::open(&worktree_path).map_err(|e| e.to_string())?;
    let head = repo.head().and_then(|h| h.peel_to_commit()).map_err(|e| e.to_string())?;
    repo.reset_default(Some(head.as_object()), [Path::new(&rel_path)]).map_err(|e| e.to_string())
}

/// Commits everything currently staged, as the next commit on HEAD.
pub fn commit(worktree_path: String, message: String) -> Result<(), String> {
    let repo = Repository::open(&worktree_path).map_err(|e| e.to_string())?;
    let mut index = repo.index().map_err(|e| e.to_string())?;
    let tree_oid = index.write_tree().map_err(|e| e.to_string())?;
    let head = repo.head().and_then(|h| h.peel_to_commit()).map_err(|e| e.to_string())?;
    if tree_oid == head.tree_id() {
        return Err("Nothing staged to commit".to_string());
    }
    let tree = repo.find_tree(tree_oid).map_err(|e| e.to_string())?;
    let sig = repo.signature().map_err(|e| e.to_string())?;
    repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&head]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Diffs a single commit against its first parent (or an empty tree, for a root commit), for
/// showing "what changed in this commit" in the git log/graph view.
pub fn diff_commit(repo_path: String, oid: String) -> Result<Vec<FileDiff>, String> {
    let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
    let commit_oid = git2::Oid::from_str(&oid).map_err(|e| e.to_string())?;
    let commit = repo.find_commit(commit_oid).map_err(|e| e.to_string())?;
    let new_tree = commit.tree().map_err(|e| e.to_string())?;

    let parent_tree = match commit.parents().next() {
        Some(parent) => parent.tree().map_err(|e| e.to_string())?,
        None => {
            let empty_oid = repo
                .treebuilder(None)
                .and_then(|b| b.write())
                .map_err(|e| e.to_string())?;
            repo.find_tree(empty_oid).map_err(|e| e.to_string())?
        }
    };

    let diff = repo
        .diff_tree_to_tree(Some(&parent_tree), Some(&new_tree), None)
        .map_err(|e| e.to_string())?;

    Ok(collect_committed(&diff, &repo, &parent_tree, &new_tree))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_diff_emphasizes_only_the_changed_word() {
        let (additions, deletions, lines) = line_diff("foo bar\n", "foo baz\n");
        assert_eq!(additions, 1);
        assert_eq!(deletions, 1);

        let delete_line = lines.iter().find(|l| l.tag == "delete").unwrap();
        let changed: Vec<&str> = delete_line
            .segments
            .iter()
            .filter(|s| s.emphasized)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(changed, vec!["bar"]);

        let insert_line = lines.iter().find(|l| l.tag == "insert").unwrap();
        let changed: Vec<&str> = insert_line
            .segments
            .iter()
            .filter(|s| s.emphasized)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(changed, vec!["baz"]);
    }

    #[test]
    fn diffs_this_repo_against_main() {
        let repo_root = env!("CARGO_MANIFEST_DIR").to_string() + "/../..";
        let result = diff_against_base(repo_root, "main".to_string())
            .expect("diff_against_base should succeed");
        // HEAD is on main itself in this dev checkout, so there should be no committed diff.
        assert!(result.files.iter().all(|f| f.section != "committed"));
    }

    /// A fresh repo with one commit (`tracked.txt`) on `main`.
    fn scratch_repo(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("madi-diff-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repository::init(&dir).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        std::fs::write(dir.join("tracked.txt"), "one\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("main", &head, true).ok();
        repo.set_head("refs/heads/main").unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn staging_moves_a_change_from_unstaged_to_staged() {
        let repo = scratch_repo("stage");
        std::fs::write(Path::new(&repo).join("tracked.txt"), "one\ntwo\n").unwrap();

        let before = diff_against_base(repo.clone(), "main".to_string()).unwrap();
        assert!(before.files.iter().any(|f| f.section == "unstaged" && f.path == "tracked.txt"));
        assert!(!before.files.iter().any(|f| f.section == "staged"));

        stage_path(repo.clone(), "tracked.txt".to_string()).unwrap();
        let staged = diff_against_base(repo.clone(), "main".to_string()).unwrap();
        assert!(staged.files.iter().any(|f| f.section == "staged" && f.path == "tracked.txt"));
        assert!(!staged.files.iter().any(|f| f.section == "unstaged"));

        unstage_path(repo.clone(), "tracked.txt".to_string()).unwrap();
        let after = diff_against_base(repo, "main".to_string()).unwrap();
        assert!(after.files.iter().any(|f| f.section == "unstaged"));
        assert!(!after.files.iter().any(|f| f.section == "staged"));
    }

    #[test]
    fn staging_a_deleted_file_removes_it_from_the_index() {
        let repo = scratch_repo("stage-delete");
        std::fs::remove_file(Path::new(&repo).join("tracked.txt")).unwrap();
        stage_path(repo.clone(), "tracked.txt".to_string()).unwrap();
        let result = diff_against_base(repo, "main".to_string()).unwrap();
        let file = result.files.iter().find(|f| f.section == "staged" && f.path == "tracked.txt").unwrap();
        assert_eq!(file.status, "deleted");
    }

    #[test]
    fn commit_writes_staged_changes_and_refuses_when_nothing_is_staged() {
        let repo = scratch_repo("commit");
        assert_eq!(commit(repo.clone(), "empty".to_string()), Err("Nothing staged to commit".to_string()));

        std::fs::write(Path::new(&repo).join("tracked.txt"), "one\ntwo\n").unwrap();
        stage_path(repo.clone(), "tracked.txt".to_string()).unwrap();
        commit(repo.clone(), "second commit".to_string()).unwrap();

        let after = diff_against_base(repo.clone(), "main".to_string()).unwrap();
        assert!(!after.files.iter().any(|f| f.section == "staged" || f.section == "unstaged"), "the commit cleared the change");

        let r = Repository::open(&repo).unwrap();
        let head = r.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().unwrap(), "second commit");
        assert_eq!(head.parent_count(), 1);
    }
}
