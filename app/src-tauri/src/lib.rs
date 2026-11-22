use tauri::{Manager, WebviewWindowBuilder};

mod commands;
mod exif;
pub mod fsops;
pub mod journal;
mod planner;
mod progress;
pub mod renamer;
mod scan;
mod session;
pub mod storage;
mod template;

pub fn run() {
    tauri::Builder::default()
        .manage(session::Session::default())
        .manage(commands::JobState::default())
        .setup(|app| {
            let storage = storage::StoragePaths::resolve(app.handle())?;
            app.manage(storage.clone());

            if let Some(window_config) = app.config().app.windows.first() {
                let mut builder = WebviewWindowBuilder::from_config(app.handle(), window_config)?;
                if let Some(webview_dir) = storage.prepare_webview_dir()? {
                    builder = builder.data_directory(webview_dir);
                }
                builder.build()?;
            }
            Ok(())
        })
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
            commands::get_storage_info,
            commands::delete_journal,
            commands::clear_journals,
            commands::open_storage_dir,
            commands::undo_job,
            commands::resume_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running Photo Renamer");
}
