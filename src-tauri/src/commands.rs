//! Thin Tauri wrappers over maditor-core; the logic itself lives there, UI-agnostic.
use maditor_core::{diff, git_log, worktree};
use std::collections::HashMap;

#[tauri::command]
pub fn list_worktrees(
    repo_path: String,
    base_branches: HashMap<String, String>,
) -> Result<Vec<worktree::WorktreeInfo>, String> {
    worktree::list_worktrees(repo_path, base_branches)
}

#[tauri::command]
pub fn check_repo(repo_path: String) -> worktree::RepoStatus {
    worktree::check_repo(repo_path)
}

#[tauri::command]
pub fn list_branches(repo_path: String) -> Result<Vec<String>, String> {
    worktree::list_branches(repo_path)
}

#[tauri::command]
pub fn remove_worktree(repo_path: String, worktree_path: String) -> Result<(), String> {
    worktree::remove_worktree(repo_path, worktree_path)
}

#[tauri::command]
pub fn diff_against_base(worktree_path: String, base_branch: String) -> Result<diff::DiffResult, String> {
    diff::diff_against_base(worktree_path, base_branch)
}

#[tauri::command]
pub fn diff_commit(repo_path: String, oid: String) -> Result<Vec<diff::FileDiff>, String> {
    diff::diff_commit(repo_path, oid)
}

#[tauri::command]
pub fn git_log(
    repo_path: String,
    branch: String,
    skip: usize,
    limit: usize,
) -> Result<Vec<git_log::CommitInfo>, String> {
    git_log::git_log(repo_path, branch, skip, limit)
}
