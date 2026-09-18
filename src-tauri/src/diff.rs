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
    /// "committed" (merge-base..HEAD) or "uncommitted" (HEAD..workdir, staged+unstaged)
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

fn collect_uncommitted(
    diff: &Diff,
    repo: &Repository,
    head_tree: &Tree,
    worktree_path: &str,
) -> Vec<FileDiff> {
    changed_paths(diff)
        .into_iter()
        .map(|(path, status)| {
            let old_bytes = blob_bytes_at(head_tree, repo, &path).unwrap_or_default();
            let new_bytes =
                std::fs::read(Path::new(worktree_path).join(&path)).unwrap_or_default();
            build_file_diff(path, status, "uncommitted", old_bytes, new_bytes)
        })
        .collect()
}

/// Diffs a worktree's HEAD against the merge-base with `base_branch` (committed changes,
/// equivalent to `git diff base_branch...HEAD`), plus HEAD against the working directory
/// (uncommitted changes, staged and unstaged combined). Each file's content is diffed
/// line-by-line with word-level emphasis inside changed lines, so the UI can render a
/// side-by-side view.
#[tauri::command]
pub fn diff_against_base(
    worktree_path: String,
    base_branch: String,
) -> Result<DiffResult, String> {
    let repo = Repository::open(&worktree_path).map_err(|e| e.to_string())?;

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
    let workdir_diff = repo
        .diff_tree_to_workdir_with_index(Some(&head_tree), None)
        .map_err(|e| e.to_string())?;

    let mut files = collect_committed(&committed_diff, &repo, &merge_base_tree, &head_tree);
    files.extend(collect_uncommitted(
        &workdir_diff,
        &repo,
        &head_tree,
        &worktree_path,
    ));

    Ok(DiffResult {
        merge_base_oid: merge_base_oid.to_string(),
        head_oid: head_commit.id().to_string(),
        files,
    })
}

/// Diffs a single commit against its first parent (or an empty tree, for a root commit), for
/// showing "what changed in this commit" in the git log/graph view.
#[tauri::command]
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
        let repo_root = env!("CARGO_MANIFEST_DIR").to_string() + "/..";
        let result = diff_against_base(repo_root, "main".to_string())
            .expect("diff_against_base should succeed");
        // HEAD is on main itself in this dev checkout, so there should be no committed diff.
        assert!(result.files.iter().all(|f| f.section == "uncommitted"));
    }
}
