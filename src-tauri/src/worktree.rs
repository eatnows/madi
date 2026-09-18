use git2::Repository;
use serde::Serialize;

#[derive(Serialize)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: String,
    pub branch: Option<String>,
    pub head_oid: Option<String>,
    pub is_main: bool,
}

fn describe(repo: &Repository, name: String, path: String, is_main: bool) -> WorktreeInfo {
    let head = repo.head().ok();
    let branch = head.as_ref().and_then(|h| h.shorthand().ok()).map(str::to_string);
    let head_oid = head.as_ref().and_then(|h| h.target()).map(|oid| oid.to_string());
    WorktreeInfo { name, path, branch, head_oid, is_main }
}

/// Lists the main working directory plus every linked worktree for the repo at `repo_path`.
#[tauri::command]
pub fn list_worktrees(repo_path: String) -> Result<Vec<WorktreeInfo>, String> {
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

    Ok(result)
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
        let worktrees = list_worktrees(repo_root).expect("list_worktrees should succeed");
        assert!(worktrees.iter().any(|w| w.is_main));
    }

    #[test]
    fn lists_main_branch_of_this_repo() {
        let repo_root = env!("CARGO_MANIFEST_DIR").to_string() + "/..";
        let branches = list_branches(repo_root).expect("list_branches should succeed");
        assert!(branches.iter().any(|b| b == "main"));
    }
}
