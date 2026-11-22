use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const SYNC_INTERVAL: u32 = 200;
pub const STATE_COMPLETED: &str = "completed";
pub const STATE_CANCELLED: &str = "cancelled";
pub const STATE_INTERRUPTED: &str = "interrupted";
pub const STATE_FAILED: &str = "failed";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Rename,
    Undo,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Record {
    Header {
        job: String,
        kind: JobKind,
        created: String,
        template: String,
        count: u32,
        #[serde(rename = "undoOf", default, skip_serializing_if = "Option::is_none")]
        undo_of: Option<String>,
    },
    Planned {
        seq: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
        from: String,
        to: String,
    },
    Intent {
        seq: u32,
        from: String,
        to: String,
        via: Option<String>,
    },
    Staged {
        seq: u32,
    },
    Done {
        seq: u32,
    },
    Failed {
        seq: u32,
        error: String,
    },
    Footer {
        state: String,
    },
}

pub struct JournalWriter {
    path: PathBuf,
    writer: BufWriter<File>,
    unsynced: u32,
}

impl JournalWriter {
    pub fn create(path: &Path, records: &[Record]) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut journal = Self {
            path: path.to_path_buf(),
            writer: BufWriter::new(file),
            unsynced: 0,
        };
        for record in records {
            serde_json::to_writer(&mut journal.writer, record)?;
            journal.writer.write_all(b"\n")?;
        }
        journal.sync()?;
        Ok(journal)
    }

    pub fn append(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().append(true).open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            writer: BufWriter::new(file),
            unsynced: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&mut self, record: &Record) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, record)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        if matches!(record, Record::Done { .. } | Record::Failed { .. }) {
            self.unsynced += 1;
            if self.unsynced >= SYNC_INTERVAL {
                self.sync()?;
            }
        }
        Ok(())
    }

    pub fn finish(&mut self, state: &str) -> io::Result<()> {
        self.write(&Record::Footer {
            state: state.to_owned(),
        })?;
        self.sync()
    }

    fn sync(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_data()?;
        self.unsynced = 0;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeqState {
    Pending,
    Intent { via: Option<PathBuf> },
    Staged { via: PathBuf },
    Done,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct PlannedEntry {
    pub seq: u32,
    pub id: Option<u32>,
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Clone, Debug)]
pub struct JournalData {
    pub path: PathBuf,
    pub job: String,
    pub kind: JobKind,
    pub created: String,
    pub template: String,
    pub undo_of: Option<String>,
    pub planned: Vec<PlannedEntry>,
    pub states: HashMap<u32, SeqState>,
    pub footer: Option<String>,
    pub ends_with_footer: bool,
}

impl JournalData {
    pub fn state(&self, seq: u32) -> SeqState {
        self.states.get(&seq).cloned().unwrap_or(SeqState::Pending)
    }

    pub fn is_incomplete(&self) -> bool {
        !self.ends_with_footer
    }
}

pub fn read(path: &Path) -> Result<JournalData, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut header = None;
    let mut planned = Vec::new();
    let mut states = HashMap::new();
    let mut footer = None;
    let mut ends_with_footer = false;

    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| error.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        // A crash can leave a half-written last line; skip anything unparsable.
        let Ok(record) = serde_json::from_str::<Record>(&line) else {
            continue;
        };
        ends_with_footer = matches!(record, Record::Footer { .. });
        match record {
            Record::Header {
                job,
                kind,
                created,
                template,
                undo_of,
                ..
            } => header = Some((job, kind, created, template, undo_of)),
            Record::Planned { seq, id, from, to } => planned.push(PlannedEntry {
                seq,
                id,
                from: from.into(),
                to: to.into(),
            }),
            Record::Intent { seq, via, .. } => {
                states.insert(
                    seq,
                    SeqState::Intent {
                        via: via.map(PathBuf::from),
                    },
                );
            }
            Record::Staged { seq } => {
                let via = match states.get(&seq) {
                    Some(SeqState::Intent { via: Some(via) }) => via.clone(),
                    _ => continue,
                };
                states.insert(seq, SeqState::Staged { via });
            }
            Record::Done { seq } => {
                states.insert(seq, SeqState::Done);
            }
            Record::Failed { seq, error } => {
                states.insert(seq, SeqState::Failed(error));
            }
            Record::Footer { state } => footer = Some(state),
        }
    }

    let (job, kind, created, template, undo_of) =
        header.ok_or_else(|| format!("{} に履歴のヘッダーがありません", path.display()))?;
    Ok(JournalData {
        path: path.to_path_buf(),
        job,
        kind,
        created,
        template,
        undo_of,
        planned,
        states,
        footer,
        ends_with_footer,
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalInfo {
    pub job: String,
    pub kind: JobKind,
    pub created: String,
    pub template: String,
    pub count: u32,
    pub done: u32,
    pub failed: u32,
    pub state: String,
    pub incomplete: bool,
    pub undo_of: Option<String>,
    pub undone_by: Option<String>,
}

pub fn journal_path(directory: &Path, job: &str) -> PathBuf {
    directory.join(format!("{job}.jsonl"))
}

pub fn read_all(directory: &Path) -> Vec<JournalData> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .filter_map(|path| read(&path).ok())
        .collect()
}

pub fn list(directory: &Path) -> Vec<JournalInfo> {
    let journals = read_all(directory);
    let undone_by = journals
        .iter()
        .filter_map(|journal| {
            journal
                .undo_of
                .as_ref()
                .map(|target| (target.clone(), journal.job.clone()))
        })
        .collect::<HashMap<_, _>>();

    let mut infos = journals
        .iter()
        .map(|journal| {
            let done = journal
                .states
                .values()
                .filter(|state| matches!(state, SeqState::Done))
                .count() as u32;
            let failed = journal
                .states
                .values()
                .filter(|state| matches!(state, SeqState::Failed(_)))
                .count() as u32;
            let incomplete = journal.is_incomplete();
            JournalInfo {
                job: journal.job.clone(),
                kind: journal.kind,
                created: journal.created.clone(),
                template: journal.template.clone(),
                count: journal.planned.len() as u32,
                done,
                failed,
                state: if incomplete {
                    STATE_INTERRUPTED.to_owned()
                } else {
                    journal.footer.clone().unwrap_or_default()
                },
                incomplete,
                undo_of: journal.undo_of.clone(),
                undone_by: undone_by.get(&journal.job).cloned(),
            }
        })
        .collect::<Vec<_>>();
    infos.sort_by(|left, right| right.created.cmp(&left.created));
    infos
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{journal_path, list, read, JobKind, JournalWriter, Record, SeqState};

    fn header() -> Record {
        Record::Header {
            job: "job-1".to_owned(),
            kind: JobKind::Rename,
            created: "2026-01-01T00:00:00".to_owned(),
            template: "IMG_{name}".to_owned(),
            count: 2,
            undo_of: None,
        }
    }

    fn planned(seq: u32) -> Record {
        Record::Planned {
            seq,
            id: Some(seq),
            from: format!("C:/p/{seq}.jpg"),
            to: format!("C:/p/IMG_{seq}.jpg"),
        }
    }

    #[test]
    fn writes_and_reads_the_record_lifecycle() {
        let directory = tempdir().unwrap();
        let path = journal_path(directory.path(), "job-1");
        let mut journal =
            JournalWriter::create(&path, &[header(), planned(1), planned(2)]).unwrap();
        journal
            .write(&Record::Intent {
                seq: 1,
                from: "C:/p/1.jpg".to_owned(),
                to: "C:/p/IMG_1.jpg".to_owned(),
                via: None,
            })
            .unwrap();
        journal.write(&Record::Done { seq: 1 }).unwrap();
        journal.finish("completed").unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#""type":"header""#));
        assert!(text.contains(r#""type":"intent""#));
        assert!(text.contains(r#""type":"done""#));
        assert!(text.contains(r#""type":"footer","state":"completed""#));

        let data = read(&path).unwrap();
        assert_eq!(data.planned.len(), 2);
        assert_eq!(data.state(1), SeqState::Done);
        assert_eq!(data.state(2), SeqState::Pending);
        assert!(!data.is_incomplete());
    }

    #[test]
    fn journals_without_a_footer_are_incomplete_and_tolerate_torn_lines() {
        let directory = tempdir().unwrap();
        let path = journal_path(directory.path(), "job-1");
        let mut journal = JournalWriter::create(&path, &[header(), planned(1)]).unwrap();
        journal.write(&Record::Done { seq: 1 }).unwrap();
        drop(journal);
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str(r#"{"type":"intent","se"#);
        fs::write(&path, text).unwrap();

        let data = read(&path).unwrap();
        assert!(data.is_incomplete());
        let infos = list(directory.path());
        assert_eq!(infos.len(), 1);
        assert!(infos[0].incomplete);
        assert_eq!(infos[0].done, 1);
    }
}
