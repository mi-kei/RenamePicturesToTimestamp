use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use chrono::Local;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    fsops,
    journal::{
        self, JobKind, JournalData, JournalWriter, PlannedEntry, Record, SeqState, STATE_CANCELLED,
        STATE_COMPLETED, STATE_FAILED, STATE_INTERRUPTED,
    },
};

#[derive(Clone, Debug)]
pub struct RenameRequest {
    pub id: Option<u32>,
    pub from: PathBuf,
    pub to: PathBuf,
}

struct Task {
    seq: u32,
    id: Option<u32>,
    from: PathBuf,
    to: PathBuf,
    staged: Option<PathBuf>,
}

pub struct ItemReport<'a> {
    pub seq: u32,
    pub id: Option<u32>,
    pub from: &'a Path,
    pub to: &'a Path,
    pub result: Result<(), String>,
    pub done: u32,
    pub total: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobReport {
    pub job: String,
    pub kind: JobKind,
    pub state: String,
    pub total: u32,
    pub ok: u32,
    pub failed: u32,
    pub error: Option<String>,
}

pub struct Job {
    id: String,
    kind: JobKind,
    writer: JournalWriter,
    tasks: Vec<Task>,
}

pub fn new_job(
    directory: &Path,
    kind: JobKind,
    template: &str,
    undo_of: Option<String>,
    requests: Vec<RenameRequest>,
) -> Result<Job, String> {
    if requests.is_empty() {
        return Err("リネームできるファイルがありません".to_owned());
    }

    let id = new_job_id();
    let tasks = requests
        .into_iter()
        .enumerate()
        .map(|(index, request)| Task {
            seq: index as u32 + 1,
            id: request.id,
            from: request.from,
            to: request.to,
            staged: None,
        })
        .collect::<Vec<_>>();

    let mut records = Vec::with_capacity(tasks.len() + 1);
    records.push(Record::Header {
        job: id.clone(),
        kind,
        created: Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        template: template.to_owned(),
        count: tasks.len() as u32,
        undo_of,
    });
    records.extend(tasks.iter().map(|task| Record::Planned {
        seq: task.seq,
        id: task.id,
        from: display(&task.from),
        to: display(&task.to),
    }));
    let writer = JournalWriter::create(&journal::journal_path(directory, &id), &records)
        .map_err(|error| format!("履歴ファイルを作成できません: {error}"))?;

    Ok(Job {
        id,
        kind,
        writer,
        tasks,
    })
}

/// Prepares the unfinished part of a job so that it can be continued.
pub fn resume(directory: &Path, job_id: &str) -> Result<Job, String> {
    let path = journal::journal_path(directory, job_id);
    let data = journal::read(&path)?;
    ensure_not_undone(directory, job_id)?;

    let mut writer = JournalWriter::append(&path)
        .map_err(|error| format!("履歴ファイルを開けません: {error}"))?;
    let mut tasks = Vec::new();
    for entry in &data.planned {
        let state = reconcile_and_record(&data, entry, &mut writer)?;
        match state {
            SeqState::Pending => tasks.push(Task {
                seq: entry.seq,
                id: entry.id,
                from: entry.from.clone(),
                to: entry.to.clone(),
                staged: None,
            }),
            SeqState::Staged { via } => tasks.push(Task {
                seq: entry.seq,
                id: entry.id,
                from: entry.from.clone(),
                to: entry.to.clone(),
                staged: Some(via),
            }),
            _ => {}
        }
    }

    Ok(Job {
        id: data.job,
        kind: data.kind,
        writer,
        tasks,
    })
}

/// Creates a new job that moves every renamed file of `job_id` back to its original name.
pub fn undo(directory: &Path, job_id: &str) -> Result<Job, String> {
    let path = journal::journal_path(directory, job_id);
    let data = journal::read(&path)?;
    if data.kind != JobKind::Rename {
        return Err("元に戻せるのはリネームの履歴だけです".to_owned());
    }
    ensure_not_undone(directory, job_id)?;

    let mut writer = JournalWriter::append(&path)
        .map_err(|error| format!("履歴ファイルを開けません: {error}"))?;
    let mut requests = Vec::new();
    for entry in data.planned.iter().rev() {
        match reconcile_and_record(&data, entry, &mut writer)? {
            SeqState::Done => requests.push(RenameRequest {
                id: entry.id,
                from: entry.to.clone(),
                to: entry.from.clone(),
            }),
            SeqState::Staged { via } => requests.push(RenameRequest {
                id: entry.id,
                from: via,
                to: entry.from.clone(),
            }),
            _ => {}
        }
    }
    if data.is_incomplete() {
        writer
            .finish(STATE_INTERRUPTED)
            .map_err(|error| format!("履歴ファイルに書き込めません: {error}"))?;
    }
    drop(writer);

    if requests.is_empty() {
        return Err("元に戻せるファイルがありません".to_owned());
    }
    new_job(
        directory,
        JobKind::Undo,
        &data.template,
        Some(job_id.to_owned()),
        requests,
    )
}

impl Job {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> JobKind {
        self.kind
    }

    pub fn total(&self) -> u32 {
        self.tasks.len() as u32
    }

    pub fn run(
        mut self,
        cancel: &AtomicBool,
        mut on_item: impl FnMut(&ItemReport<'_>),
    ) -> JobReport {
        let tasks = std::mem::take(&mut self.tasks);
        let total = tasks.len() as u32;
        let mut done = 0;
        let mut ok = 0;
        let mut failed = 0;
        let mut cancelled = false;
        let mut journal_error: Option<String> = None;

        let mut pending = tasks
            .iter()
            .filter(|task| task.staged.is_none())
            .map(|task| key(&task.from))
            .collect::<HashSet<_>>();
        let (mut deferred, direct): (Vec<Task>, Vec<Task>) =
            tasks.into_iter().partition(|task| task.staged.is_some());

        for task in direct {
            if cancel.load(Ordering::SeqCst) {
                cancelled = true;
                break;
            }
            pending.remove(&key(&task.from));

            // The target still belongs to a file that has not moved yet: park this file
            // under a temporary name and finish it after everything else has moved.
            let via = pending
                .contains(&key(&task.to))
                .then(|| temp_path(&task.to, &self.id, task.seq));
            if let Err(error) = self.writer.write(&Record::Intent {
                seq: task.seq,
                from: display(&task.from),
                to: display(&task.to),
                via: via.as_deref().map(display),
            }) {
                journal_error = Some(error.to_string());
                break;
            }

            let target = via.as_deref().unwrap_or(&task.to);
            let result = fsops::rename_no_replace(&task.from, target).map_err(describe);
            match (result, via) {
                (Ok(()), Some(via)) => {
                    if let Err(error) = self.writer.write(&Record::Staged { seq: task.seq }) {
                        journal_error = Some(error.to_string());
                    }
                    deferred.push(Task {
                        staged: Some(via),
                        ..task
                    });
                    if journal_error.is_some() {
                        break;
                    }
                    continue;
                }
                (result, _) => {
                    let record = match &result {
                        Ok(()) => Record::Done { seq: task.seq },
                        Err(error) => Record::Failed {
                            seq: task.seq,
                            error: error.clone(),
                        },
                    };
                    if let Err(error) = self.writer.write(&record) {
                        journal_error = Some(error.to_string());
                    }
                    done += 1;
                    if result.is_ok() {
                        ok += 1;
                    } else {
                        failed += 1;
                    }
                    on_item(&ItemReport {
                        seq: task.seq,
                        id: task.id,
                        from: &task.from,
                        to: &task.to,
                        result,
                        done,
                        total,
                    });
                    if journal_error.is_some() {
                        break;
                    }
                }
            }
        }

        // Parked files are always finished, even after a cancellation.
        for task in deferred {
            let via = task.staged.clone().expect("deferred tasks are staged");
            let result = match fsops::rename_no_replace(&via, &task.to) {
                Ok(()) => Ok(()),
                Err(error) => match fsops::rename_no_replace(&via, &task.from) {
                    Ok(()) => Err(format!("{}（元の名前に戻しました）", describe(error))),
                    Err(back) => Err(format!(
                        "{}（一時的な名前 {} のままです: {}）",
                        describe(error),
                        via.display(),
                        describe(back)
                    )),
                },
            };
            let record = match &result {
                Ok(()) => Record::Done { seq: task.seq },
                Err(error) => Record::Failed {
                    seq: task.seq,
                    error: error.clone(),
                },
            };
            if let Err(error) = self.writer.write(&record) {
                journal_error.get_or_insert(error.to_string());
            }
            done += 1;
            if result.is_ok() {
                ok += 1;
            } else {
                failed += 1;
            }
            on_item(&ItemReport {
                seq: task.seq,
                id: task.id,
                from: &task.from,
                to: &task.to,
                result,
                done,
                total,
            });
        }

        let state = if journal_error.is_some() {
            STATE_FAILED
        } else if cancelled {
            STATE_CANCELLED
        } else {
            STATE_COMPLETED
        };
        if let Err(error) = self.writer.finish(state) {
            journal_error.get_or_insert(error.to_string());
        }

        JobReport {
            job: self.id,
            kind: self.kind,
            state: state.to_owned(),
            total,
            ok,
            failed,
            error: journal_error.map(|error| format!("履歴ファイルに書き込めません: {error}")),
        }
    }
}

fn ensure_not_undone(directory: &Path, job_id: &str) -> Result<(), String> {
    if journal::read_all(directory)
        .iter()
        .any(|journal| journal.undo_of.as_deref() == Some(job_id))
    {
        return Err("この実行はすでに元に戻す処理が行われています".to_owned());
    }
    Ok(())
}

/// Compares the journal with the file system to learn what happened to an entry
/// whose last record was written right before a crash.
fn reconcile(state: SeqState, entry: &PlannedEntry) -> SeqState {
    let from_exists = exists(&entry.from);
    let to_exists = exists(&entry.to);
    let missing = || SeqState::Failed("ファイルが見つかりません".to_owned());

    match state {
        SeqState::Intent { via: None } => {
            if !from_exists && to_exists {
                SeqState::Done
            } else if from_exists {
                SeqState::Pending
            } else {
                missing()
            }
        }
        SeqState::Intent { via: Some(via) } | SeqState::Staged { via } => {
            if exists(&via) {
                SeqState::Staged { via }
            } else if from_exists {
                SeqState::Pending
            } else if to_exists {
                SeqState::Done
            } else {
                missing()
            }
        }
        other => other,
    }
}

fn reconcile_and_record(
    data: &JournalData,
    entry: &PlannedEntry,
    writer: &mut JournalWriter,
) -> Result<SeqState, String> {
    let recorded = data.state(entry.seq);
    let state = reconcile(recorded.clone(), entry);
    if state != recorded {
        let record = match &state {
            SeqState::Done => Some(Record::Done { seq: entry.seq }),
            SeqState::Failed(error) => Some(Record::Failed {
                seq: entry.seq,
                error: error.clone(),
            }),
            SeqState::Staged { via } => Some(Record::Intent {
                seq: entry.seq,
                from: display(&entry.from),
                to: display(&entry.to),
                via: Some(display(via)),
            }),
            _ => None,
        };
        if let Some(record) = record {
            writer
                .write(&record)
                .map_err(|error| format!("履歴ファイルに書き込めません: {error}"))?;
            if matches!(state, SeqState::Staged { .. }) {
                writer
                    .write(&Record::Staged { seq: entry.seq })
                    .map_err(|error| format!("履歴ファイルに書き込めません: {error}"))?;
            }
        }
    }
    Ok(state)
}

fn new_job_id() -> String {
    let unique = Uuid::new_v4().simple().to_string();
    format!("{}-{}", Local::now().format("%Y%m%d-%H%M%S"), &unique[..8])
}

fn temp_path(target: &Path, job: &str, seq: u32) -> PathBuf {
    let directory = target.parent().unwrap_or_else(|| Path::new(""));
    let short = &job[job.len().saturating_sub(8)..];
    let mut attempt = 0;
    loop {
        let name = if attempt == 0 {
            format!(".prtmp-{short}-{seq}")
        } else {
            format!(".prtmp-{short}-{seq}-{attempt}")
        };
        let candidate = directory.join(name);
        if !exists(&candidate) {
            return candidate;
        }
        attempt += 1;
    }
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn describe(error: io::Error) -> String {
    match error.kind() {
        io::ErrorKind::AlreadyExists => "変更先に同じ名前のファイルがあります".to_owned(),
        io::ErrorKind::NotFound => "ファイルが見つかりません".to_owned(),
        io::ErrorKind::PermissionDenied => {
            "ファイルにアクセスできません（使用中か権限がありません）".to_owned()
        }
        _ => error.to_string(),
    }
}
