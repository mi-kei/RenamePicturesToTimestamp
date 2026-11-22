use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const MIN_EMIT_INTERVAL: Duration = Duration::from_millis(100);
const MAX_RECENT_LINES: usize = 500;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: ProgressPhase,
    pub done: u32,
    pub total: u32,
    pub current: Option<String>,
    pub ok: u32,
    pub skipped: u32,
    pub errors: u32,
    pub rate: f32,
    pub recent: Vec<LogLine>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgressPhase {
    Scan,
    Exif,
    Rename,
    Undo,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

pub struct ProgressEmitter {
    app: AppHandle,
    last_emitted: Option<Instant>,
}

impl ProgressEmitter {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            last_emitted: None,
        }
    }

    pub fn is_due(&self) -> bool {
        self.last_emitted
            .map_or(true, |last| last.elapsed() >= MIN_EMIT_INTERVAL)
    }

    pub fn emit(&mut self, progress: &Progress, force: bool) -> Result<bool, tauri::Error> {
        let should_emit = force || self.is_due();

        if should_emit {
            self.app.emit("job-progress", progress)?;
            self.last_emitted = Some(Instant::now());
        }

        Ok(should_emit)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Error,
}

/// Accumulates per-item results and sends them to the UI in throttled batches.
pub struct ProgressTracker {
    emitter: ProgressEmitter,
    phase: ProgressPhase,
    total: u32,
    done: u32,
    ok: u32,
    skipped: u32,
    errors: u32,
    started: Instant,
    current: Option<String>,
    recent: Vec<LogLine>,
}

impl ProgressTracker {
    pub fn new(app: AppHandle, phase: ProgressPhase, total: u32) -> Self {
        Self {
            emitter: ProgressEmitter::new(app),
            phase,
            total,
            done: 0,
            ok: 0,
            skipped: 0,
            errors: 0,
            started: Instant::now(),
            current: None,
            recent: Vec::new(),
        }
    }

    pub fn record(&mut self, current: String, outcome: Outcome, message: String) {
        self.done += 1;
        let level = match outcome {
            Outcome::Ok => {
                self.ok += 1;
                "info"
            }
            Outcome::Error => {
                self.errors += 1;
                "error"
            }
        };
        self.current = Some(current);
        self.recent.push(LogLine {
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            level: level.to_owned(),
            message,
        });
        if self.recent.len() > MAX_RECENT_LINES {
            let overflow = self.recent.len() - MAX_RECENT_LINES;
            self.recent.drain(..overflow);
        }
        self.flush(false);
    }

    pub fn flush(&mut self, force: bool) {
        if !force && !self.emitter.is_due() {
            return;
        }
        let elapsed = self.started.elapsed().as_secs_f32();
        let progress = Progress {
            phase: self.phase.clone(),
            done: self.done,
            total: self.total,
            current: self.current.clone(),
            ok: self.ok,
            skipped: self.skipped,
            errors: self.errors,
            rate: if elapsed > 0.0 {
                self.done as f32 / elapsed
            } else {
                0.0
            },
            recent: std::mem::take(&mut self.recent),
        };
        if let Err(error) = self.emitter.emit(&progress, true) {
            eprintln!("failed to emit progress: {error}");
        }
    }
}
