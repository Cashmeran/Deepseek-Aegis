// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod state;

fn main() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .manage(state::SessionState::new())
    .invoke_handler(tauri::generate_handler![
      commands::session::session_list,
      commands::session::recent_cwds,
      commands::client_event::client_event,
    ])
    .run(tauri::generate_context!())
    .expect("error while running aegis desktop");
}
