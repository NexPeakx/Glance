pub mod selection;
mod selection_bridge;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            selection_bridge::init(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            selection_bridge::selection_check_permission,
            selection_bridge::selection_get_config,
            selection_bridge::selection_update_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
