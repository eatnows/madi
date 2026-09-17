use git2::{Diff, Patch, Repository};
use serde::Serialize;

#[derive(Serialize)]
pub struct FileDiff {
    pub path: String,
    pub status: String,
    pub additions: usize,
    pub deletions: usize,
    /// "committed" (merge-base..HEAD) or "uncommitted" (HEAD..workdir, staged+unstaged)
    pub section: &'static str,
    pub patch: String,
}

#[derive(Serialize)]
pub struct DiffResult {
    pub merge_base_oid: String,
    pub head_oid: String,
    pub files: Vec<FileDiff>,
}

fn status_label(status: git2::Delta) -> &'static str {
    match status {
        git2::Delta::Added => "added",
        git2::Delta::Deleted => "deleted",
        git2::Delta::Modified => "modified",
        git2::Delta::Renamed => "renamed",
        git2::Delta::Copied => "copied",
        _ => "other",
    }
}

fn collect_file_diffs(diff: &Diff, section: &'static str) -> Result<Vec<FileDiff>, git2::Error> {
    let mut out = Vec::new();
    for idx in 0..diff.deltas().len() {
        let Some(mut patch) = Patch::from_diff(diff, idx)? else {
            continue;
        };
        let (path, status) = {
            let delta = patch.delta();
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            (path, status_label(delta.status()))
        };
        let (_, additions, deletions) = patch.line_stats()?;
        let patch_text = patch.to_buf()?.as_str().unwrap_or("").to_owned();
        out.push(FileDiff {
            path,
            status: status.to_string(),
            additions,
            deletions,
            section,
            patch: patch_text,
        });
    }
    Ok(out)
}

/// Diffs a worktree's HEAD against the merge-base with `base_branch` (committed changes,
/// equivalent to `git diff base_branch...HEAD`), plus HEAD against the working directory
/// (uncommitted changes, staged and unstaged combined).
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

    let committed_diff = repo
        .diff_tree_to_tree(
            Some(&merge_base_commit.tree().map_err(|e| e.to_string())?),
            Some(&head_commit.tree().map_err(|e| e.to_string())?),
            None,
        )
        .map_err(|e| e.to_string())?;
    let workdir_diff = repo
        .diff_tree_to_workdir_with_index(Some(&head_commit.tree().map_err(|e| e.to_string())?), None)
        .map_err(|e| e.to_string())?;

    let mut files = collect_file_diffs(&committed_diff, "committed").map_err(|e| e.to_string())?;
    files.extend(collect_file_diffs(&workdir_diff, "uncommitted").map_err(|e| e.to_string())?);

    Ok(DiffResult {
        merge_base_oid: merge_base_oid.to_string(),
        head_oid: head_commit.id().to_string(),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs_this_repo_against_main() {
        let repo_root = env!("CARGO_MANIFEST_DIR").to_string() + "/..";
        let result = diff_against_base(repo_root, "main".to_string())
            .expect("diff_against_base should succeed");
        // HEAD is on main itself in this dev checkout, so there should be no committed diff.
        assert!(result.files.iter().all(|f| f.section == "uncommitted"));
    }
}
