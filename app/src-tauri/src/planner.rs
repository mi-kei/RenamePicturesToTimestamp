use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    session::{Item, ItemStatus},
    template::{RenderContext, Template},
};

const MAX_SUFFIX: usize = 9999;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Resolve {
    #[default]
    Suffix,
    Skip,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExtCase {
    #[default]
    Keep,
    Lower,
    Upper,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlanOptions {
    pub on_conflict: Resolve,
    pub ext_case: ExtCase,
    pub skip_already_named: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            on_conflict: Resolve::Suffix,
            ext_case: ExtCase::Keep,
            skip_already_named: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub total: u32,
    pub ok: u32,
    pub suffixed: u32,
    pub conflict: u32,
    pub no_timestamp: u32,
    pub unchanged: u32,
    pub excluded: u32,
    pub errors: u32,
    pub done: u32,
}

pub type DirectoryCache = HashMap<PathBuf, HashSet<String>>;

struct Proposal {
    index: usize,
    name: String,
}

enum Outcome {
    Assigned { name: String, suffixed: bool },
    Conflict(String),
}

pub fn plan(
    items: &mut [Item],
    template: &Template,
    options: &PlanOptions,
    directories: &mut DirectoryCache,
) -> PlanSummary {
    let mut candidates = Vec::new();
    for (index, item) in items.iter_mut().enumerate() {
        item.planned = None;
        if matches!(item.status, ItemStatus::Done) {
            continue;
        }
        if item.excluded {
            item.status = ItemStatus::Excluded;
            continue;
        }
        if item.manual_name.is_none() {
            if item.exif.is_none() {
                if let Some(error) = &item.exif_error {
                    item.status = ItemStatus::Error(format!("EXIF を読み取れません: {error}"));
                    continue;
                }
            }
            if template.uses_date() && item.taken_at().is_none() {
                item.status = ItemStatus::NoTimestamp;
                continue;
            }
        }
        candidates.push(index);
    }

    candidates.sort_by(|left, right| {
        let left = &items[*left];
        let right = &items[*right];
        (left.taken_at(), &left.path).cmp(&(right.taken_at(), &right.path))
    });

    let mut proposals = Vec::with_capacity(candidates.len());
    for (position, index) in candidates.into_iter().enumerate() {
        let item = &mut items[index];
        let name = match &item.manual_name {
            Some(manual) => manual.trim().to_owned(),
            None => {
                let stem = item.stem();
                let camera = item.camera();
                let context = RenderContext {
                    taken_at: item.taken_at(),
                    name: &stem,
                    camera: camera.as_deref(),
                    seq: position + 1,
                };
                if options.skip_already_named && template.matches_existing(&stem, &context) {
                    item.status = ItemStatus::Unchanged;
                    continue;
                }
                let Some(rendered) = template.render(&context) else {
                    item.status = ItemStatus::NoTimestamp;
                    continue;
                };
                with_extension(rendered, item.ext().as_deref(), options.ext_case)
            }
        };

        if name == item.file_name() {
            item.status = ItemStatus::Unchanged;
            continue;
        }
        if let Err(reason) = validate_file_name(&name) {
            item.status = ItemStatus::Error(reason);
            continue;
        }
        proposals.push(Proposal { index, name });
    }

    let outcomes = resolve_conflicts(items, &proposals, options.on_conflict, directories);
    for (proposal, outcome) in proposals.iter().zip(outcomes) {
        let item = &mut items[proposal.index];
        match outcome {
            Outcome::Assigned { name, suffixed } => {
                item.planned = Some(name);
                item.status = if suffixed {
                    ItemStatus::Suffixed
                } else {
                    ItemStatus::Ok
                };
            }
            Outcome::Conflict(reason) => {
                item.planned = Some(proposal.name.clone());
                item.status = ItemStatus::Conflict(reason);
            }
        }
    }

    summarize(items)
}

pub fn summarize(items: &[Item]) -> PlanSummary {
    let mut summary = PlanSummary {
        total: items.len() as u32,
        ..PlanSummary::default()
    };
    for item in items {
        match item.status {
            ItemStatus::Ok => summary.ok += 1,
            ItemStatus::Suffixed => {
                summary.ok += 1;
                summary.suffixed += 1;
            }
            ItemStatus::Conflict(_) => summary.conflict += 1,
            ItemStatus::NoTimestamp => summary.no_timestamp += 1,
            ItemStatus::Unchanged => summary.unchanged += 1,
            ItemStatus::Excluded => summary.excluded += 1,
            ItemStatus::Error(_) => summary.errors += 1,
            ItemStatus::Done => summary.done += 1,
            ItemStatus::Pending => {}
        }
    }
    summary
}

fn resolve_conflicts(
    items: &[Item],
    proposals: &[Proposal],
    resolve: Resolve,
    directories: &mut DirectoryCache,
) -> Vec<Outcome> {
    let mut pinned: HashMap<usize, String> = HashMap::new();

    loop {
        let mut blocked: HashMap<&Path, HashSet<String>> = HashMap::new();
        for proposal in proposals {
            let dir = items[proposal.index].dir();
            blocked
                .entry(dir)
                .or_insert_with(|| existing_names(directories, dir).clone());
        }
        for (position, proposal) in proposals.iter().enumerate() {
            if pinned.contains_key(&position) {
                continue;
            }
            let item = &items[proposal.index];
            if let Some(names) = blocked.get_mut(item.dir()) {
                names.remove(&item.file_name().to_lowercase());
            }
        }

        let mut claimed: HashMap<&Path, HashSet<String>> = HashMap::new();
        let mut outcomes = Vec::with_capacity(proposals.len());
        let mut new_pins = false;

        for (position, proposal) in proposals.iter().enumerate() {
            if let Some(reason) = pinned.get(&position) {
                outcomes.push(Outcome::Conflict(reason.clone()));
                continue;
            }

            let dir = items[proposal.index].dir();
            let blocked = &blocked[dir];
            let claimed = claimed.entry(dir).or_default();
            let is_free = |name: &str| {
                let key = name.to_lowercase();
                !blocked.contains(&key) && !claimed.contains(&key)
            };

            let outcome = if is_free(&proposal.name) {
                Outcome::Assigned {
                    name: proposal.name.clone(),
                    suffixed: false,
                }
            } else {
                let reason = if blocked.contains(&proposal.name.to_lowercase()) {
                    "同じフォルダに同じ名前のファイルがあります".to_owned()
                } else {
                    "ほかのファイルの変更後の名前と重複しています".to_owned()
                };
                match resolve {
                    Resolve::Skip => Outcome::Conflict(reason),
                    Resolve::Suffix => match find_suffixed_name(&proposal.name, is_free) {
                        Some(name) => Outcome::Assigned {
                            name,
                            suffixed: true,
                        },
                        None => {
                            Outcome::Conflict(format!("{reason}（空いている連番がありません）"))
                        }
                    },
                }
            };

            match &outcome {
                Outcome::Assigned { name, .. } => {
                    claimed.insert(name.to_lowercase());
                }
                Outcome::Conflict(reason) => {
                    pinned.insert(position, reason.clone());
                    new_pins = true;
                }
            }
            outcomes.push(outcome);
        }

        if !new_pins {
            return outcomes;
        }
    }
}

fn find_suffixed_name(name: &str, is_free: impl Fn(&str) -> bool) -> Option<String> {
    let (stem, extension) = split_name(name);
    (1..=MAX_SUFFIX)
        .map(|number| match extension {
            Some(extension) => format!("{stem}_{number:03}.{extension}"),
            None => format!("{stem}_{number:03}"),
        })
        .find(|candidate| validate_file_name(candidate).is_ok() && is_free(candidate))
}

fn existing_names<'a>(directories: &'a mut DirectoryCache, dir: &Path) -> &'a HashSet<String> {
    directories.entry(dir.to_path_buf()).or_insert_with(|| {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn split_name(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(index) if index > 0 => (&name[..index], Some(&name[index + 1..])),
        _ => (name, None),
    }
}

fn with_extension(stem: String, extension: Option<&str>, ext_case: ExtCase) -> String {
    match extension {
        Some(extension) if !extension.is_empty() => {
            let extension = match ext_case {
                ExtCase::Keep => extension.to_owned(),
                ExtCase::Lower => extension.to_lowercase(),
                ExtCase::Upper => extension.to_uppercase(),
            };
            format!("{stem}.{extension}")
        }
        _ => stem,
    }
}

pub fn validate_file_name(name: &str) -> Result<(), String> {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    if name.trim().is_empty() {
        return Err("ファイル名が空です".to_owned());
    }
    if name == "." || name == ".." {
        return Err("ファイル名に使えない名前です".to_owned());
    }
    if name
        .chars()
        .any(|character| character.is_control() || "<>:\"/\\|?*".contains(character))
    {
        return Err(
            "ファイル名に使えない文字（< > : \" / \\ | ? * など）が含まれています".to_owned(),
        );
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("ファイル名の末尾にピリオドや空白は使えません".to_owned());
    }
    let device = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_uppercase();
    if RESERVED.contains(&device.as_str()) {
        return Err(format!("Windows の予約名（{device}）は使えません"));
    }
    if name.len() > 255 || name.encode_utf16().count() > 255 {
        return Err("ファイル名が長すぎます".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use chrono::NaiveDate;
    use tempfile::tempdir;

    use super::{plan, validate_file_name, DirectoryCache, ExtCase, PlanOptions, Resolve};
    use crate::{
        exif::{ExifInfo, ExifSource},
        session::{Item, ItemStatus},
        template::{Template, DEFAULT_TEMPLATE},
    };

    fn item(id: u32, dir: &Path, name: &str, second: Option<u32>) -> Item {
        let path = dir.join(name);
        fs::write(&path, b"image").unwrap();
        let mut item = Item::new(id, path);
        item.exif = second.map(|second| ExifInfo {
            taken_at: NaiveDate::from_ymd_opt(2023, 2, 23)
                .unwrap()
                .and_hms_opt(12, 53, second),
            source: ExifSource::Native,
            make: Some("Sony".to_owned()),
            model: Some("ILCE-7M4".to_owned()),
        });
        item
    }

    fn run(items: &mut [Item], template: &str, options: PlanOptions) -> super::PlanSummary {
        plan(
            items,
            &Template::parse(template).unwrap(),
            &options,
            &mut DirectoryCache::new(),
        )
    }

    #[test]
    fn default_plan_matches_the_python_script_output() {
        let directory = tempdir().unwrap();
        let mut items = vec![item(0, directory.path(), "7M400001.ARW", Some(12))];

        let summary = run(&mut items, DEFAULT_TEMPLATE, PlanOptions::default());

        assert_eq!(summary.ok, 1);
        assert_eq!(items[0].status, ItemStatus::Ok);
        assert_eq!(
            items[0].planned.as_deref(),
            Some("IMG_20230223_125312_7M400001.ARW")
        );
    }

    #[test]
    fn duplicate_names_get_numeric_suffixes_or_are_skipped() {
        let directory = tempdir().unwrap();
        let make_items = || {
            vec![
                item(0, directory.path(), "a.jpg", Some(12)),
                item(1, directory.path(), "b.jpg", Some(12)),
                item(2, directory.path(), "c.jpg", Some(12)),
            ]
        };

        let mut items = make_items();
        let summary = run(&mut items, "{date:YYYYMMDD_HHmmss}", PlanOptions::default());
        assert_eq!(summary.ok, 3);
        assert_eq!(summary.suffixed, 2);
        let mut names = items
            .iter()
            .map(|item| item.planned.clone().unwrap())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            [
                "20230223_125312.jpg",
                "20230223_125312_001.jpg",
                "20230223_125312_002.jpg"
            ]
        );

        let mut items = make_items();
        let options = PlanOptions {
            on_conflict: Resolve::Skip,
            ..PlanOptions::default()
        };
        let summary = run(&mut items, "{date:YYYYMMDD_HHmmss}", options);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.conflict, 2);
    }

    #[test]
    fn detects_existing_files_outside_the_session() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("20230223_125312.JPG"), b"other").unwrap();
        let mut items = vec![item(0, directory.path(), "a.jpg", Some(12))];
        let options = PlanOptions {
            on_conflict: Resolve::Skip,
            ..PlanOptions::default()
        };

        let summary = run(&mut items, "{date:YYYYMMDD_HHmmss}", options);

        assert_eq!(summary.conflict, 1);
        assert!(matches!(items[0].status, ItemStatus::Conflict(_)));
    }

    #[test]
    fn detects_duplicates_that_differ_only_by_case() {
        let directory = tempdir().unwrap();
        let mut items = vec![
            item(0, directory.path(), "a.jpg", Some(1)),
            item(1, directory.path(), "b.jpg", Some(2)),
        ];
        items[0].manual_name = Some("Photo.jpg".to_owned());
        items[1].manual_name = Some("photo.JPG".to_owned());
        let options = PlanOptions {
            on_conflict: Resolve::Skip,
            ..PlanOptions::default()
        };

        let summary = run(&mut items, DEFAULT_TEMPLATE, options);

        assert_eq!(summary.ok, 1);
        assert_eq!(summary.conflict, 1);
    }

    #[test]
    fn already_named_unchanged_and_missing_dates_are_not_renamed() {
        let directory = tempdir().unwrap();
        let mut items = vec![
            item(
                0,
                directory.path(),
                "IMG_20230223_125312_7M400001.ARW",
                Some(12),
            ),
            item(1, directory.path(), "no-date.jpg", None),
            item(2, directory.path(), "keep.jpg", Some(12)),
        ];
        items[2].manual_name = Some("keep.jpg".to_owned());

        let summary = run(&mut items, DEFAULT_TEMPLATE, PlanOptions::default());

        assert_eq!(items[0].status, ItemStatus::Unchanged);
        assert_eq!(items[1].status, ItemStatus::NoTimestamp);
        assert_eq!(items[2].status, ItemStatus::Unchanged);
        assert_eq!(summary.unchanged, 2);
        assert_eq!(summary.no_timestamp, 1);
    }

    #[test]
    fn chained_renames_within_the_session_are_not_conflicts() {
        let directory = tempdir().unwrap();
        let mut items = vec![
            item(0, directory.path(), "a.jpg", Some(1)),
            item(1, directory.path(), "b.jpg", Some(2)),
        ];
        items[0].manual_name = Some("b.jpg".to_owned());
        items[1].manual_name = Some("c.jpg".to_owned());
        let options = PlanOptions {
            on_conflict: Resolve::Skip,
            ..PlanOptions::default()
        };

        let summary = run(&mut items, DEFAULT_TEMPLATE, options);

        assert_eq!(summary.ok, 2);
    }

    #[test]
    fn a_file_that_stays_in_place_blocks_its_name() {
        let directory = tempdir().unwrap();
        let mut items = vec![
            item(0, directory.path(), "a.jpg", Some(1)),
            item(1, directory.path(), "b.jpg", Some(2)),
            item(2, directory.path(), "c.jpg", Some(3)),
        ];
        items[0].manual_name = Some("b.jpg".to_owned());
        items[1].manual_name = Some("c.jpg".to_owned());
        items[2].excluded = true;
        let options = PlanOptions {
            on_conflict: Resolve::Skip,
            ..PlanOptions::default()
        };

        let summary = run(&mut items, DEFAULT_TEMPLATE, options);

        assert_eq!(summary.excluded, 1);
        assert_eq!(summary.conflict, 2);
    }

    #[test]
    fn extension_case_and_sequence_numbers_are_applied() {
        let directory = tempdir().unwrap();
        let mut items = vec![
            item(0, directory.path(), "b.JPG", Some(2)),
            item(1, directory.path(), "a.JPG", Some(1)),
        ];
        let options = PlanOptions {
            ext_case: ExtCase::Lower,
            ..PlanOptions::default()
        };

        run(&mut items, "trip_{seq:3}_{camera}", options);

        assert_eq!(items[1].planned.as_deref(), Some("trip_001_ILCE-7M4.jpg"));
        assert_eq!(items[0].planned.as_deref(), Some("trip_002_ILCE-7M4.jpg"));
    }

    #[test]
    fn invalid_manual_names_are_errors() {
        for name in ["a:b.jpg", "CON.jpg", "lpt1", "name.", "", "a/b.jpg"] {
            assert!(
                validate_file_name(name).is_err(),
                "{name:?} should be invalid"
            );
        }
        assert!(validate_file_name("IMG_20230223.a.b.jpg").is_ok());

        let directory = tempdir().unwrap();
        let mut items = vec![item(0, directory.path(), "a.jpg", Some(1))];
        items[0].manual_name = Some("CON.jpg".to_owned());
        let summary = run(&mut items, DEFAULT_TEMPLATE, PlanOptions::default());
        assert_eq!(summary.errors, 1);
    }
}
