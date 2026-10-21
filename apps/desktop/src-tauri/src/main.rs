#[tauri::command]
fn core_version() -> &'static str {
    modman_core::VERSION
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![core_version])
        .run(tauri::generate_context!())
        .expect("error while running modman");
}
