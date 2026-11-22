use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use chrono::NaiveDateTime;
use serde::Serialize;

use crate::{
    exif::{ExifInfo, ExifSource, MetadataResult},
    planner::{self, DirectoryCache, PlanOptions, PlanSummary},
    template::{Template, DEFAULT_TEMPLATE},
};

const MAX_PAGE_SIZE: u32 = 2000;

#[derive(Default)]
pub struct Session {
    state: Mutex<SessionState>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum ItemStatus {
    Pending,
    Ok,
    Suffixed,
    Conflict(String),
    NoTimestamp,
    Unchanged,
    Excluded,
    Error(String),
    Done,
}

impl ItemStatus {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ok => "ok",
            Self::Suffixed => "suffixed",
            Self::Conflict(_) => "conflict",
            Self::NoTimestamp => "noTimestamp",
            Self::Unchanged => "unchanged",
            Self::Excluded => "excluded",
            Self::Error(_) => "error",
            Self::Done => "done",
        }
    }

    pub fn is_renamable(&self) -> bool {
        matches!(self, Self::Ok | Self::Suffixed)
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: u32,
    pub path: PathBuf,
    pub exif: Option<ExifInfo>,
    pub exif_error: Option<String>,
    pub excluded: bool,
    pub manual_name: Option<String>,
    pub planned: Option<String>,
    pub status: ItemStatus,
}

impl Item {
    pub fn new(id: u32, path: PathBuf) -> Self {
        Self {
            id,
            path,
            exif: None,
            exif_error: None,
            excluded: false,
            manual_name: None,
            planned: None,
            status: ItemStatus::Pending,
        }
    }

    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or_else(|| Path::new(""))
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn stem(&self) -> String {
        self.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn ext(&self) -> Option<String> {
        self.path
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned())
    }

    pub fn taken_at(&self) -> Option<NaiveDateTime> {
        self.exif.as_ref().and_then(|exif| exif.taken_at)
    }

    pub fn camera(&self) -> Option<String> {
        let exif = self.exif.as_ref()?;
        exif.model.clone().or_else(|| exif.make.clone())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: u32,
    pub original_name: String,
    pub directory: String,
    pub new_name: Option<String>,
    pub taken_at: Option<String>,
    pub status: ItemStatus,
    pub excluded: bool,
    pub manual: bool,
    pub make: Option<String>,
    pub model: Option<String>,
    pub source: Option<ExifSource>,
    pub exif_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowPage {
    pub total: u32,
    pub offset: u32,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug)]
pub struct PlannedRename {
    pub id: u32,
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Default)]
struct SessionState {
    items: Vec<Item>,
    indices: HashMap<u32, usize>,
    paths: HashMap<String, u32>,
    next_id: u32,
    plan: Option<(Template, PlanOptions)>,
    directories: DirectoryCache,
}

impl SessionState {
    /// Finds an item by id when it still points at `path`, otherwise by path alone.
    fn find(&self, id: Option<u32>, path: &Path) -> Option<usize> {
        let key = crate::scan::path_key(path);
        if let Some(index) = id.and_then(|id| self.indices.get(&id).copied()) {
            if crate::scan::path_key(&self.items[index].path) == key {
                return Some(index);
            }
        }
        self.paths
            .get(&key)
            .and_then(|id| self.indices.get(id).copied())
    }

    fn replan(&mut self) -> Result<PlanSummary, String> {
        let (template, options) = match &self.plan {
            Some((template, options)) => (template.clone(), options.clone()),
            None => (Template::parse(DEFAULT_TEMPLATE)?, PlanOptions::default()),
        };
        Ok(planner::plan(
            &mut self.items,
            &template,
            &options,
            &mut self.directories,
        ))
    }
}

impl Session {
    fn lock(&self) -> Result<MutexGuard<'_, SessionState>, String> {
        self.state
            .lock()
            .map_err(|_| "セッションの状態を利用できません".to_owned())
    }

    pub fn add_paths(&self, paths: Vec<PathBuf>) -> Result<Vec<Item>, String> {
        let mut state = self.lock()?;
        let mut added = Vec::new();

        for path in paths {
            let key = crate::scan::path_key(&path);
            if state.paths.contains_key(&key) {
                continue;
            }

            let next_id = state
                .next_id
                .checked_add(1)
                .ok_or_else(|| "読み込めるファイル数の上限を超えました".to_owned())?;
            let item = Item::new(state.next_id, path);
            let index = state.items.len();
            state.paths.insert(key, item.id);
            state.indices.insert(item.id, index);
            state.items.push(item.clone());
            state.next_id = next_id;
            added.push(item);
        }

        state.directories.clear();
        Ok(added)
    }

    pub fn clear(&self) -> Result<(), String> {
        let mut state = self.lock()?;
        let plan = state.plan.take();
        *state = SessionState::default();
        state.plan = plan;
        Ok(())
    }

    pub fn items(&self) -> Result<Vec<Item>, String> {
        self.lock().map(|state| state.items.clone())
    }

    pub fn apply_metadata(&self, results: &[MetadataResult]) -> Result<(), String> {
        let mut state = self.lock()?;

        for result in results {
            let index = state
                .indices
                .get(&result.id)
                .copied()
                .ok_or_else(|| format!("不明な項目です: {}", result.id))?;
            let item = &mut state.items[index];
            item.exif = result.exif.clone();
            item.exif_error = result.error.clone();
        }

        Ok(())
    }

    pub fn build_plan(&self, template: &str, options: PlanOptions) -> Result<PlanSummary, String> {
        let template = Template::parse(template)?;
        let mut state = self.lock()?;
        state.plan = Some((template, options));
        state.replan()
    }

    pub fn set_override(
        &self,
        id: u32,
        excluded: Option<bool>,
        manual_name: Option<String>,
    ) -> Result<PlanSummary, String> {
        let mut state = self.lock()?;
        let index = state
            .indices
            .get(&id)
            .copied()
            .ok_or_else(|| format!("不明な項目です: {id}"))?;
        let item = &mut state.items[index];
        if let Some(excluded) = excluded {
            item.excluded = excluded;
        }
        if let Some(manual_name) = manual_name {
            let manual_name = manual_name.trim();
            item.manual_name = (!manual_name.is_empty()).then(|| manual_name.to_owned());
        }
        state.replan()
    }

    pub fn summary(&self) -> Result<PlanSummary, String> {
        self.lock().map(|state| planner::summarize(&state.items))
    }

    pub fn template_source(&self) -> Result<String, String> {
        self.lock().map(|state| {
            state
                .plan
                .as_ref()
                .map(|(template, _)| template.source().to_owned())
                .unwrap_or_else(|| DEFAULT_TEMPLATE.to_owned())
        })
    }

    pub fn get_rows(
        &self,
        offset: u32,
        limit: u32,
        filter: Option<&[String]>,
    ) -> Result<RowPage, String> {
        let state = self.lock()?;
        let filter = filter
            .filter(|kinds| !kinds.is_empty())
            .map(|kinds| kinds.iter().map(String::as_str).collect::<HashSet<_>>());
        let matching = state
            .items
            .iter()
            .filter(|item| {
                filter
                    .as_ref()
                    .map_or(true, |kinds| kinds.contains(item.status.kind()))
            })
            .collect::<Vec<_>>();
        let limit = limit.min(MAX_PAGE_SIZE) as usize;
        let rows = matching
            .iter()
            .skip(offset as usize)
            .take(limit)
            .map(|item| to_row(item))
            .collect();

        Ok(RowPage {
            total: matching.len() as u32,
            offset,
            rows,
        })
    }

    pub fn planned_renames(&self) -> Result<Vec<PlannedRename>, String> {
        let state = self.lock()?;
        Ok(state
            .items
            .iter()
            .filter(|item| item.status.is_renamable())
            .filter_map(|item| {
                item.planned.as_ref().map(|name| PlannedRename {
                    id: item.id,
                    from: item.path.clone(),
                    to: item.dir().join(name),
                })
            })
            .collect())
    }

    /// Records that the file at `from` now lives at `to`.
    pub fn apply_move(
        &self,
        id: Option<u32>,
        from: &Path,
        to: &Path,
        status: ItemStatus,
    ) -> Result<(), String> {
        let mut state = self.lock()?;
        let Some(index) = state.find(id, from) else {
            return Ok(());
        };
        let old_key = crate::scan::path_key(&state.items[index].path);
        let item_id = state.items[index].id;
        state.paths.remove(&old_key);
        state.paths.insert(crate::scan::path_key(to), item_id);
        let item = &mut state.items[index];
        item.path = to.to_path_buf();
        item.manual_name = None;
        item.planned = None;
        item.status = status;
        Ok(())
    }

    pub fn set_item_status(
        &self,
        id: Option<u32>,
        path: &Path,
        status: ItemStatus,
    ) -> Result<(), String> {
        let mut state = self.lock()?;
        if let Some(index) = state.find(id, path) {
            state.items[index].status = status;
        }
        Ok(())
    }

    pub fn invalidate_directories(&self) -> Result<(), String> {
        self.lock().map(|mut state| state.directories.clear())
    }

    pub fn refresh_plan(&self) -> Result<PlanSummary, String> {
        let mut state = self.lock()?;
        state.directories.clear();
        state.replan()
    }
}

fn to_row(item: &Item) -> Row {
    let exif = item.exif.as_ref();
    Row {
        id: item.id,
        original_name: item.file_name(),
        directory: item.dir().to_string_lossy().into_owned(),
        new_name: item.planned.clone(),
        taken_at: item
            .taken_at()
            .map(|taken_at| taken_at.format("%Y-%m-%d %H:%M:%S").to_string()),
        status: item.status.clone(),
        excluded: item.excluded,
        manual: item.manual_name.is_some(),
        make: exif.and_then(|exif| exif.make.clone()),
        model: exif.and_then(|exif| exif.model.clone()),
        source: exif.map(|exif| exif.source.clone()),
        exif_error: item.exif_error.clone(),
    }
}
