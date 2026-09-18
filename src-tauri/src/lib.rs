mod diff;
mod worktree;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            worktree::list_worktrees,
            worktree::list_branches,
            diff::diff_against_base
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
