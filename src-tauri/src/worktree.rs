use git2::{Oid, Repository};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: String,
    pub branch: Option<String>,
    pub head_oid: Option<String>,
    pub is_main: bool,
    /// Commits on this worktree's HEAD not on its pinned base branch, and vice versa.
    /// `None` when `base_branches` had no entry for this worktree's path, or it didn't resolve.
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
}

fn describe(repo: &Repository, name: String, path: String, is_main: bool) -> (WorktreeInfo, Option<Oid>) {
    let head = repo.head().ok();
    let branch = head.as_ref().and_then(|h| h.shorthand().ok()).map(str::to_string);
    let head_oid = head.as_ref().and_then(|h| h.target());
    let info = WorktreeInfo {
        name,
        path,
        branch,
        head_oid: head_oid.map(|oid| oid.to_string()),
        is_main,
        ahead: None,
        behind: None,
    };
    (info, head_oid)
}

/// Lists the main working directory plus every linked worktree for the repo at `repo_path`.
/// `base_branches` maps a worktree's path to the base branch it's pinned to (the caller owns
/// that pinning; git itself has no notion of "which branch this was forked from"). Entries
/// missing from the map get `ahead`/`behind` of `None`.
#[tauri::command]
pub fn list_worktrees(
    repo_path: String,
    base_branches: HashMap<String, String>,
) -> Result<Vec<WorktreeInfo>, String> {
    let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
    let mut result = Vec::new();

    if let Some(workdir) = repo.workdir() {
        result.push(describe(&repo, "(main)".to_string(), workdir.to_string_lossy().into_owned(), true));
    }

    for name in repo.worktrees().map_err(|e| e.to_string())?.iter().flatten().flatten() {
        let wt = repo.find_worktree(name).map_err(|e| e.to_string())?;
        let wt_repo = Repository::open_from_worktree(&wt).map_err(|e| e.to_string())?;
        let path = wt.path().to_string_lossy().into_owned();
        result.push(describe(&wt_repo, name.to_string(), path, false));
    }

    Ok(result
        .into_iter()
        .map(|(mut info, head_oid)| {
            let base_oid = base_branches.get(&info.path).and_then(|b| {
                repo.find_branch(b, git2::BranchType::Local)
                    .ok()
                    .and_then(|br| br.get().target())
            });
            if let (Some(head_oid), Some(base_oid)) = (head_oid, base_oid) {
                if let Ok((ahead, behind)) = repo.graph_ahead_behind(head_oid, base_oid) {
                    info.ahead = Some(ahead);
                    info.behind = Some(behind);
                }
            }
            info
        })
        .collect())
}

/// Lists local branch names, for populating a base-branch picker.
#[tauri::command]
pub fn list_branches(repo_path: String) -> Result<Vec<String>, String> {
    let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
    let branches = repo
        .branches(Some(git2::BranchType::Local))
        .map_err(|e| e.to_string())?;
    let mut names = Vec::new();
    for branch in branches {
        let (branch, _) = branch.map_err(|e| e.to_string())?;
        if let Some(name) = branch.name().map_err(|e| e.to_string())? {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_main_worktree_of_this_repo() {
        let repo_root = env!("CARGO_MANIFEST_DIR").to_string() + "/..";
        let worktrees = list_worktrees(repo_root, HashMap::new()).expect("list_worktrees should succeed");
        assert!(worktrees.iter().any(|w| w.is_main));
    }

    #[test]
    fn computes_ahead_behind_against_pinned_base_branch() {
        let repo_root = env!("CARGO_MANIFEST_DIR").to_string() + "/..";
        let main_workdir = Repository::open(&repo_root).unwrap().workdir().unwrap().to_string_lossy().into_owned();
        let mut base_branches = HashMap::new();
        base_branches.insert(main_workdir, "main".to_string());

        let worktrees = list_worktrees(repo_root, base_branches).expect("list_worktrees should succeed");
        let main_entry = worktrees.iter().find(|w| w.is_main).unwrap();
        // HEAD of this dev checkout is main itself, so it's neither ahead nor behind main.
        assert_eq!(main_entry.ahead, Some(0));
        assert_eq!(main_entry.behind, Some(0));
    }

    #[test]
    fn lists_main_branch_of_this_repo() {
        let repo_root = env!("CARGO_MANIFEST_DIR").to_string() + "/..";
        let branches = list_branches(repo_root).expect("list_branches should succeed");
        assert!(branches.iter().any(|b| b == "main"));
    }
}
