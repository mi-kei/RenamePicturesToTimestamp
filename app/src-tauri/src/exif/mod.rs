mod exiftool;
mod native;

use std::path::PathBuf;

use chrono::{DateTime, NaiveDateTime};
use rayon::prelude::*;
use serde::Serialize;

use crate::scan::path_key;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExifInfo {
    pub taken_at: Option<NaiveDateTime>,
    pub source: ExifSource,
    pub make: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExifSource {
    Native,
    ExifTool,
}

#[derive(Clone, Debug)]
pub struct MetadataResult {
    pub id: u32,
    pub exif: Option<ExifInfo>,
    pub error: Option<String>,
}

pub struct MetadataBatch {
    pub results: Vec<MetadataResult>,
    pub warning: Option<String>,
}

pub fn read_metadata<F>(items: &[(u32, PathBuf)], mut report_progress: F) -> MetadataBatch
where
    F: FnMut(u32, u32, Option<String>),
{
    const PARALLEL_CHUNK_SIZE: usize = 256;
    const EXIFTOOL_CHUNK_SIZE: usize = 500;

    let total = items.len();
    let mut native_results = Vec::with_capacity(total);

    for (chunk_index, chunk) in items.chunks(PARALLEL_CHUNK_SIZE).enumerate() {
        let chunk_results = chunk
            .par_iter()
            .map(|(_, path)| native::read(path))
            .collect::<Vec<_>>();
        native_results.extend(chunk_results);

        let done = ((chunk_index + 1) * PARALLEL_CHUNK_SIZE).min(total);
        let current = chunk.last().map(|(_, path)| path.display().to_string());
        report_progress(done as u32, total as u32, current);
    }

    let fallback_indices = native_results
        .iter()
        .enumerate()
        .filter_map(|(index, result)| result.as_ref().err().map(|_| index))
        .collect::<Vec<_>>();
    let mut results = items
        .iter()
        .zip(native_results)
        .map(|((id, _), result)| match result {
            Ok(exif) => MetadataResult {
                id: *id,
                exif,
                error: None,
            },
            Err(error) => MetadataResult {
                id: *id,
                exif: None,
                error: Some(error),
            },
        })
        .collect::<Vec<_>>();

    if fallback_indices.is_empty() {
        if total == 0 {
            report_progress(0, 0, None);
        }
        return MetadataBatch {
            results,
            warning: None,
        };
    }

    let mut tool = match exiftool::ExifToolProcess::start() {
        Ok(tool) => tool,
        Err(error) => {
            return MetadataBatch {
                results,
                warning: Some(format!(
                    "ExifTool is unavailable; some files could not be read: {error}"
                )),
            };
        }
    };

    let mut completed_fallbacks = 0;
    for batch in fallback_indices.chunks(EXIFTOOL_CHUNK_SIZE) {
        let mut query_indices = Vec::with_capacity(batch.len());
        for index in batch {
            let path = &items[*index].1;
            let path_string = path.to_string_lossy();
            if path_string.contains('\n') || path_string.contains('\r') {
                let native_error = results[*index]
                    .error
                    .take()
                    .unwrap_or_else(|| "Native EXIF reader failed".to_owned());
                results[*index].error = Some(format!(
                    "{native_error}; ExifTool cannot process a path containing a newline"
                ));
            } else {
                query_indices.push(*index);
            }
        }

        if query_indices.is_empty() {
            completed_fallbacks += batch.len();
            report_progress(
                (total + completed_fallbacks) as u32,
                (total + fallback_indices.len()) as u32,
                batch
                    .last()
                    .map(|index| items[*index].1.display().to_string()),
            );
            continue;
        }

        let paths = query_indices
            .iter()
            .map(|index| items[*index].1.clone())
            .collect::<Vec<_>>();
        let records = match tool.read_batch(&paths) {
            Ok(records) => records,
            Err(error) => {
                for index in &query_indices {
                    let native_error = results[*index]
                        .error
                        .take()
                        .unwrap_or_else(|| "Native EXIF reader failed".to_owned());
                    results[*index].error =
                        Some(format!("{native_error}; ExifTool fallback failed: {error}"));
                }
                completed_fallbacks += batch.len();
                report_progress(
                    (total + completed_fallbacks) as u32,
                    (total + fallback_indices.len()) as u32,
                    batch
                        .last()
                        .map(|index| items[*index].1.display().to_string()),
                );
                continue;
            }
        };

        for index in &query_indices {
            let path = &items[*index].1;
            if let Some(exif) = records.get(&path_key(path)) {
                results[*index].exif = Some(exif.clone());
                results[*index].error = None;
            } else {
                let native_error = results[*index]
                    .error
                    .take()
                    .unwrap_or_else(|| "Native EXIF reader failed".to_owned());
                results[*index].error = Some(format!(
                    "{native_error}; ExifTool returned no metadata for {}",
                    path.display()
                ));
            }
        }

        completed_fallbacks += batch.len();
        report_progress(
            (total + completed_fallbacks) as u32,
            (total + fallback_indices.len()) as u32,
            batch
                .last()
                .map(|index| items[*index].1.display().to_string()),
        );
    }

    MetadataBatch {
        results,
        warning: None,
    }
}

pub(crate) fn parse_datetime(value: &str) -> Option<NaiveDateTime> {
    let value = value.trim().trim_end_matches('\0');
    for format in [
        "%Y:%m:%d %H:%M:%S",
        "%Y:%m:%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
    ] {
        if let Ok(datetime) = NaiveDateTime::parse_from_str(value, format) {
            return Some(datetime);
        }
    }

    for format in ["%Y:%m:%d %H:%M:%S%:z", "%Y-%m-%d %H:%M:%S%:z"] {
        if let Ok(datetime) = DateTime::parse_from_str(value, format) {
            return Some(datetime.naive_local());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::read_metadata;

    #[test]
    fn missing_exif_is_reported_as_no_timestamp_without_error() {
        let directory = tempdir().unwrap();
        let photo = directory.path().join("plain.jpg");
        fs::write(&photo, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap();

        let batch = read_metadata(&[(7, photo)], |_, _, _| {});

        assert_eq!(batch.results.len(), 1);
        assert_eq!(batch.results[0].id, 7);
        assert!(batch.results[0].error.is_none());
        assert!(batch.results[0].exif.is_none());
    }
}
