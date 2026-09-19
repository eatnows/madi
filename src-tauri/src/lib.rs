mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_worktrees,
            commands::list_branches,
            commands::check_repo,
            commands::remove_worktree,
            commands::diff_against_base,
            commands::diff_commit,
            commands::git_log
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
