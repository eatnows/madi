//! Scanning a project's git repository: its branches and worktrees, with each worktree's base
//! branch pinned (defaulting to `main`). Runs off the UI thread, so it is plain blocking code.
use std::collections::HashMap;

use maditor_git::worktree::{self, RepoStatus, WorktreeInfo};

/// Why a registered folder can't be used as a git project.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Issue {
    NotARepo,
    Missing,
}

impl Issue {
    pub fn label(self) -> &'static str {
        match self {
            Issue::NotARepo => "Not a git repository",
            Issue::Missing => "Folder not found",
        }
    }
}

pub struct Scan {
    pub worktrees: Vec<WorktreeInfo>,
    /// worktree path -> base branch
    pub pins: HashMap<String, String>,
    pub branches: Vec<String>,
}

pub enum ScanOutcome {
    Issue(Issue),
    Loaded(Scan),
}

/// `pins` are the base branches already chosen; worktrees without one get the default branch.
pub fn scan_repo(repo: String, mut pins: HashMap<String, String>) -> Result<ScanOutcome, String> {
    match worktree::check_repo(repo.clone()) {
        RepoStatus::Ok => {}
        RepoStatus::NotARepo => return Ok(ScanOutcome::Issue(Issue::NotARepo)),
        RepoStatus::Missing => return Ok(ScanOutcome::Issue(Issue::Missing)),
    }
    let branches = worktree::list_branches(repo.clone())?;
    let default = if branches.iter().any(|b| b == "main") {
        "main".to_string()
    } else {
        branches.first().cloned().unwrap_or_default()
    };
    for wt in worktree::list_worktrees(repo.clone(), HashMap::new())? {
        pins.entry(wt.path).or_insert_with(|| default.clone());
    }
    let worktrees = worktree::list_worktrees(repo, pins.clone())?;
    Ok(ScanOutcome::Loaded(Scan { worktrees, pins, branches }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, process::Command};

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn scans_worktrees_and_pins_new_ones_to_main_but_keeps_existing_pins() {
        let root = std::env::temp_dir().join("maditor-project-test-scan");
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "T"]);
        std::fs::write(repo.join("a.txt"), "x\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        let wt = root.join("wt");
        git(&repo, &["worktree", "add", "-q", "-b", "feature", wt.to_str().unwrap()]);

        let ScanOutcome::Loaded(fresh) = scan_repo(repo.to_string_lossy().into_owned(), HashMap::new()).unwrap() else {
            panic!("expected a loaded repo");
        };
        assert_eq!(fresh.worktrees.len(), 2);
        assert_eq!(fresh.branches, ["feature", "main"]);
        assert!(fresh.pins.values().all(|b| b == "main"));

        let wt_path = fresh.worktrees.iter().find(|w| !w.is_main).unwrap().path.clone();
        let saved = HashMap::from([(wt_path.clone(), "feature".to_string())]);
        let ScanOutcome::Loaded(kept) = scan_repo(repo.to_string_lossy().into_owned(), saved).unwrap() else {
            panic!("expected a loaded repo");
        };
        assert_eq!(kept.pins[&wt_path], "feature", "an existing pin isn't overwritten by the default");
    }

    #[test]
    fn reports_a_plain_folder_and_a_missing_one() {
        let dir = std::env::temp_dir().join("maditor-project-test-plain");
        std::fs::create_dir_all(&dir).unwrap();
        let outcome = |p: &Path| match scan_repo(p.to_string_lossy().into_owned(), HashMap::new()).unwrap() {
            ScanOutcome::Issue(i) => i,
            ScanOutcome::Loaded(_) => panic!("not a repo"),
        };
        assert_eq!(outcome(&dir), Issue::NotARepo);
        assert_eq!(outcome(&dir.join("nope")), Issue::Missing);
        assert_eq!(Issue::NotARepo.label(), "Not a git repository");
    }
}
