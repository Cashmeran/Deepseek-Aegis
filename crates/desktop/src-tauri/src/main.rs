// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code)]

mod commands;
mod computer;
mod events;
mod project;
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
      commands::session::list_skills,
      commands::session::load_projects,
      commands::session::load_project_sessions,
      commands::session::register_project,
      commands::session::read_session_file,
      commands::session::save_session_messages,
      commands::session::delete_session,
      commands::session::delete_project,
      commands::session::check_existing_project,
      commands::session::get_log_dir,
      commands::session::open_log_dir,
      commands::session::get_mcp_config,
      commands::session::save_mcp_config,
      commands::session::open_mcp_config_dir,
      commands::session::get_compaction_config,
      commands::session::save_compaction_config,
      commands::session::get_computer_use_enabled,
      commands::session::set_computer_use_enabled,
      commands::client_event::client_event,
      commands::client_event::get_config,
      project::project_init,
      project::project_open,
      project::project_scan,
      project::project_check,
      project::project_list_rules,
      project::project_save_rule,
      project::project_list_sessions,
      project::list_project_files,
    ])
    .run(tauri::generate_context!())
    .expect("error while running aegis desktop");
}
