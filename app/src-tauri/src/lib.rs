mod commands;
mod exif;
pub mod fsops;
pub mod journal;
mod planner;
mod progress;
pub mod renamer;
mod scan;
mod session;
mod template;

pub fn run() {
    tauri::Builder::default()
        .manage(session::Session::default())
        .manage(commands::JobState::default())
        .invoke_handler(tauri::generate_handler![
            commands::scan_paths,
            commands::read_metadata,
            commands::build_plan,
            commands::get_rows,
            commands::set_override,
            commands::clear_session,
            commands::get_summary,
            commands::execute_rename,
            commands::cancel_job,
            commands::list_journals,
            commands::undo_job,
            commands::resume_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running Photo Renamer");
}
