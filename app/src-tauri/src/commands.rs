use std::{
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    exif,
    journal::{self, JobKind, JournalInfo},
    planner::{PlanOptions, PlanSummary},
    progress::{Outcome, Progress, ProgressEmitter, ProgressPhase, ProgressTracker},
    renamer::{self, Job, RenameRequest},
    scan::{self, ScanSummary, ScannedFile, DEFAULT_EXTENSIONS},
    session::{ItemStatus, RowPage, Session},
    storage::{StorageInfo, StoragePaths},
};

#[tauri::command(async)]
pub fn scan_paths(
    paths: Vec<String>,
    recursive: bool,
    extensions: Vec<String>,
    app: AppHandle,
    session: State<'_, Session>,
) -> Result<ScanSummary, String> {
    let roots = paths.into_iter().map(Into::into).collect::<Vec<_>>();
    let extensions = if extensions.is_empty() {
        DEFAULT_EXTENSIONS
            .iter()
            .map(|extension| (*extension).to_owned())
            .collect()
    } else {
        extensions
    };
    let mut emitter = ProgressEmitter::new(app);
    let scanned = scan::scan_paths(&roots, recursive, &extensions, |done, path| {
        let progress = Progress {
            phase: ProgressPhase::Scan,
            done: done as u32,
            total: 0,
            current: Some(path.display().to_string()),
            ok: done as u32,
            skipped: 0,
            errors: 0,
            rate: 0.0,
            recent: Vec::new(),
        };
        let _ = emitter.emit(&progress, false);
    });
    let found = scanned.paths.len();
    let added = session.add_paths(scanned.paths)?;
    let progress = Progress {
        phase: ProgressPhase::Scan,
        done: found as u32,
        total: found as u32,
        current: None,
        ok: added.len() as u32,
        skipped: (scanned.skipped + found.saturating_sub(added.len())) as u32,
        errors: scanned.errors.len() as u32,
        rate: 0.0,
        recent: Vec::new(),
    };
    let _ = emitter.emit(&progress, true);

    Ok(ScanSummary {
        found,
        added: added.len(),
        skipped: scanned.skipped + found.saturating_sub(added.len()),
        files: added
            .into_iter()
            .map(|item| ScannedFile {
                id: item.id,
                path: item.path.to_string_lossy().into_owned(),
            })
            .collect(),
        errors: scanned.errors,
    })
}

#[tauri::command(async)]
pub fn build_plan(
    template: String,
    options: Option<PlanOptions>,
    session: State<'_, Session>,
) -> Result<PlanSummary, String> {
    session.build_plan(&template, options.unwrap_or_default())
}

#[tauri::command(async)]
pub fn get_rows(
    offset: u32,
    limit: u32,
    filter: Option<Vec<String>>,
    session: State<'_, Session>,
) -> Result<RowPage, String> {
    session.get_rows(offset, limit, filter.as_deref())
}

#[tauri::command(async)]
pub fn set_override(
    id: u32,
    excluded: Option<bool>,
    manual_name: Option<String>,
    session: State<'_, Session>,
) -> Result<PlanSummary, String> {
    session.set_override(id, excluded, manual_name)
}

#[tauri::command]
pub fn clear_session(session: State<'_, Session>) -> Result<(), String> {
    session.clear()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataSummary {
    pub total: usize,
    pub with_timestamp: usize,
    pub no_timestamp: usize,
    pub errors: usize,
    pub warning: Option<String>,
}

#[tauri::command(async)]
pub fn read_metadata(
    app: AppHandle,
    session: State<'_, Session>,
) -> Result<MetadataSummary, String> {
    let items = session.items()?;
    let paths = items
        .iter()
        .map(|item| (item.id, item.path.clone()))
        .collect::<Vec<_>>();
    let mut emitter = ProgressEmitter::new(app);
    let batch = exif::read_metadata(&paths, |done, total, current| {
        let progress = Progress {
            phase: ProgressPhase::Exif,
            done,
            total,
            current,
            ok: 0,
            skipped: 0,
            errors: 0,
            rate: 0.0,
            recent: Vec::new(),
        };
        let _ = emitter.emit(&progress, done >= total);
    });
    session.apply_metadata(&batch.results)?;

    let total = batch.results.len();
    let errors = batch
        .results
        .iter()
        .filter(|result| result.error.is_some())
        .count();
    let with_timestamp = batch
        .results
        .iter()
        .filter(|result| {
            result
                .exif
                .as_ref()
                .and_then(|info| info.taken_at)
                .is_some()
        })
        .count();

    Ok(MetadataSummary {
        total,
        with_timestamp,
        no_timestamp: total.saturating_sub(with_timestamp + errors),
        errors,
        warning: batch.warning,
    })
}

#[derive(Default)]
pub struct JobState {
    running: Mutex<Option<RunningJob>>,
}

struct RunningJob {
    id: String,
    cancel: Arc<AtomicBool>,
}

fn start_job(
    app: AppHandle,
    jobs: &JobState,
    storage: &StoragePaths,
    prepare: impl FnOnce(&Path) -> Result<Job, String>,
) -> Result<String, String> {
    let mut running = jobs
        .running
        .lock()
        .map_err(|_| "処理の状態を利用できません".to_owned())?;
    if running.is_some() {
        return Err("別の処理を実行中です".to_owned());
    }

    let job = prepare(&storage.journals_dir)?;
    let id = job.id().to_owned();
    let cancel = Arc::new(AtomicBool::new(false));
    *running = Some(RunningJob {
        id: id.clone(),
        cancel: cancel.clone(),
    });
    drop(running);

    thread::spawn(move || run_job(app, job, cancel));
    Ok(id)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn run_job(app: AppHandle, job: Job, cancel: Arc<AtomicBool>) {
    let kind = job.kind();
    let phase = match kind {
        JobKind::Rename => ProgressPhase::Rename,
        JobKind::Undo => ProgressPhase::Undo,
    };
    let mut tracker = ProgressTracker::new(app.clone(), phase, job.total());
    tracker.flush(true);
    let session = app.state::<Session>();

    let report = job.run(&cancel, |item| {
        let from = file_name(item.from);
        let to = file_name(item.to);
        match &item.result {
            Ok(()) => {
                let status = match kind {
                    JobKind::Rename => ItemStatus::Done,
                    JobKind::Undo => ItemStatus::Pending,
                };
                let _ = session.apply_move(item.id, item.from, item.to, status);
                tracker.record(from.clone(), Outcome::Ok, format!("{from} → {to}"));
            }
            Err(error) => {
                let _ =
                    session.set_item_status(item.id, item.from, ItemStatus::Error(error.clone()));
                tracker.record(
                    from.clone(),
                    Outcome::Error,
                    format!("{from} → {to}: {error}"),
                );
            }
        }
    });

    if kind == JobKind::Undo {
        let _ = session.refresh_plan();
    } else {
        let _ = session.invalidate_directories();
    }
    tracker.flush(true);

    if let Ok(mut running) = app.state::<JobState>().running.lock() {
        *running = None;
    }
    if let Err(error) = app.emit("job-finished", &report) {
        eprintln!("failed to emit job result: {error}");
    }
}

#[tauri::command(async)]
pub fn execute_rename(
    app: AppHandle,
    session: State<'_, Session>,
    jobs: State<'_, JobState>,
    storage: State<'_, StoragePaths>,
) -> Result<String, String> {
    let template = session.template_source()?;
    let requests = session
        .planned_renames()?
        .into_iter()
        .map(|rename| RenameRequest {
            id: Some(rename.id),
            from: rename.from,
            to: rename.to,
        })
        .collect::<Vec<_>>();
    start_job(app, &jobs, &storage, |directory| {
        renamer::new_job(directory, JobKind::Rename, &template, None, requests)
    })
}

#[tauri::command]
pub fn cancel_job(jobs: State<'_, JobState>) -> Result<bool, String> {
    let running = jobs
        .running
        .lock()
        .map_err(|_| "処理の状態を利用できません".to_owned())?;
    Ok(match running.as_ref() {
        Some(job) => {
            job.cancel.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    })
}

#[tauri::command(async)]
pub fn list_journals(
    jobs: State<'_, JobState>,
    storage: State<'_, StoragePaths>,
) -> Result<Vec<JournalInfo>, String> {
    let running = jobs
        .running
        .lock()
        .map_err(|_| "処理の状態を利用できません".to_owned())?
        .as_ref()
        .map(|job| job.id.clone());
    let mut journals = journal::list(&storage.journals_dir);
    for info in &mut journals {
        if running.as_deref() == Some(info.job.as_str()) {
            info.state = "running".to_owned();
            info.incomplete = false;
        }
    }
    Ok(journals)
}

#[tauri::command]
pub fn get_storage_info(storage: State<'_, StoragePaths>) -> StorageInfo {
    storage.info()
}

#[tauri::command(async)]
pub fn delete_journal(
    job: String,
    jobs: State<'_, JobState>,
    storage: State<'_, StoragePaths>,
) -> Result<(), String> {
    let running = jobs
        .running
        .lock()
        .map_err(|_| "処理の状態を利用できません".to_owned())?;
    if running.as_ref().is_some_and(|current| current.id == job) {
        return Err("実行中の履歴は削除できません".to_owned());
    }
    journal::delete(&storage.journals_dir, &job)?;
    Ok(())
}

#[tauri::command(async)]
pub fn clear_journals(
    jobs: State<'_, JobState>,
    storage: State<'_, StoragePaths>,
) -> Result<u32, String> {
    let running = jobs
        .running
        .lock()
        .map_err(|_| "処理の状態を利用できません".to_owned())?;
    if running.is_some() {
        return Err("処理の実行中は履歴を削除できません".to_owned());
    }
    journal::clear(&storage.journals_dir)
}

#[tauri::command(async)]
pub fn open_storage_dir(storage: State<'_, StoragePaths>) -> Result<(), String> {
    let target = storage
        .open_directory_target()
        .ok_or_else(|| "保存先フォルダが見つかりません".to_owned())?;
    open_directory(&target)
}

fn open_directory(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = Command::new("explorer");
        cmd.arg(path);
        cmd
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut cmd = Command::new("open");
        cmd.arg(path);
        cmd
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(path);
        cmd
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("フォルダを開けません ({}): {error}", path.display()))
}

#[tauri::command(async)]
pub fn undo_job(
    job: String,
    app: AppHandle,
    jobs: State<'_, JobState>,
    storage: State<'_, StoragePaths>,
) -> Result<String, String> {
    start_job(app, &jobs, &storage, |directory| {
        renamer::undo(directory, &job)
    })
}

#[tauri::command(async)]
pub fn resume_job(
    job: String,
    app: AppHandle,
    jobs: State<'_, JobState>,
    storage: State<'_, StoragePaths>,
) -> Result<String, String> {
    start_job(app, &jobs, &storage, |directory| {
        renamer::resume(directory, &job)
    })
}

#[tauri::command(async)]
pub fn get_summary(session: State<'_, Session>) -> Result<PlanSummary, String> {
    session.summary()
}
