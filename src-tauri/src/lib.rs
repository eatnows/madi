mod diff;
mod git_log;
mod worktree;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            worktree::list_worktrees,
            worktree::list_branches,
            worktree::check_repo,
            worktree::remove_worktree,
            diff::diff_against_base,
            diff::diff_commit,
            git_log::git_log
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
