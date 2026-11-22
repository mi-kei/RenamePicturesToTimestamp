use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use jwalk::WalkDir;
use serde::Serialize;

pub const DEFAULT_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "tif", "tiff", "png", "webp", "dng", "arw", "cr2", "cr3", "nef", "nrw", "orf",
    "raf", "rw2", "pef", "srw", "hif", "heic", "heif",
];

#[derive(Default)]
pub struct ScanResult {
    pub paths: Vec<PathBuf>,
    pub skipped: usize,
    pub errors: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedFile {
    pub id: u32,
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub found: usize,
    pub added: usize,
    pub skipped: usize,
    pub files: Vec<ScannedFile>,
    pub errors: Vec<String>,
}

pub fn scan_paths<F>(
    roots: &[PathBuf],
    recursive: bool,
    extensions: &[String],
    mut on_file: F,
) -> ScanResult
where
    F: FnMut(usize, &Path),
{
    let allowed_extensions = extensions
        .iter()
        .map(|extension| extension.trim_start_matches('.').to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut result = ScanResult::default();
    let mut seen = HashSet::new();

    for root in roots {
        let root = match dunce::canonicalize(root) {
            Ok(path) => path,
            Err(error) => {
                result.errors.push(format!("{}: {error}", root.display()));
                continue;
            }
        };
        let metadata = match fs::metadata(&root) {
            Ok(metadata) => metadata,
            Err(error) => {
                result.errors.push(format!("{}: {error}", root.display()));
                continue;
            }
        };

        if is_hidden(&root, &metadata) {
            result.skipped += 1;
            continue;
        }

        if metadata.is_file() {
            add_file(
                root,
                &allowed_extensions,
                &mut seen,
                &mut result,
                &mut on_file,
            );
            continue;
        }

        if !metadata.is_dir() {
            result
                .errors
                .push(format!("{} is not a file or directory", root.display()));
            continue;
        }

        let mut walker = WalkDir::new(&root).skip_hidden(true);
        if !recursive {
            walker = walker.max_depth(1);
        }

        for entry in walker.into_iter() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    result.errors.push(error.to_string());
                    continue;
                }
            };
            if entry.depth() == 0 || !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    result.errors.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            if is_hidden(&path, &metadata) {
                result.skipped += 1;
                continue;
            }

            add_file(
                path,
                &allowed_extensions,
                &mut seen,
                &mut result,
                &mut on_file,
            );
        }
    }

    result
}

pub(crate) fn path_key(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    }
}

fn add_file<F>(
    path: PathBuf,
    allowed_extensions: &HashSet<String>,
    seen: &mut HashSet<String>,
    result: &mut ScanResult,
    on_file: &mut F,
) where
    F: FnMut(usize, &Path),
{
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    if !extension.is_some_and(|extension| allowed_extensions.contains(&extension)) {
        result.skipped += 1;
        return;
    }

    let key = path_key(&path);
    if !seen.insert(key) {
        result.skipped += 1;
        return;
    }

    result.paths.push(path);
    on_file(
        result.paths.len(),
        result.paths.last().expect("file was just added"),
    );
}

fn is_hidden(path: &Path, metadata: &fs::Metadata) -> bool {
    let dot_hidden = path
        .file_name()
        .map(|name| name.to_string_lossy().starts_with('.'))
        .unwrap_or(false);

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        dot_hidden || metadata.file_attributes() & 0x2 != 0
    }

    #[cfg(not(windows))]
    {
        let _ = metadata;
        dot_hidden
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use tempfile::tempdir;

    use super::scan_paths;

    fn scan(roots: &[PathBuf], recursive: bool) -> super::ScanResult {
        scan_paths(
            roots,
            recursive,
            &["jpg".to_owned(), "arw".to_owned()],
            |_, _| {},
        )
    }

    #[test]
    fn scans_recursive_files_and_filters_hidden_and_unsupported_files() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("photos");
        let nested = root.join("nested");
        let hidden = root.join(".private");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&hidden).unwrap();
        fs::write(root.join("photo.JPG"), b"image").unwrap();
        fs::write(nested.join("raw.ARW"), b"image").unwrap();
        fs::write(root.join("notes.txt"), b"text").unwrap();
        fs::write(root.join(".hidden.jpg"), b"image").unwrap();
        fs::write(hidden.join("secret.jpg"), b"image").unwrap();

        let result = scan(&[root], true);

        assert!(result.errors.is_empty());
        assert_eq!(result.paths.len(), 2);
        assert!(result.paths.iter().any(|path| path.ends_with("photo.JPG")));
        assert!(result.paths.iter().any(|path| path.ends_with("raw.ARW")));
    }

    #[test]
    fn non_recursive_scan_only_includes_direct_children() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("photos");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("root.jpg"), b"image").unwrap();
        fs::write(nested.join("child.jpg"), b"image").unwrap();

        let result = scan(&[root], false);

        assert_eq!(result.paths.len(), 1);
        assert!(result.paths[0].ends_with("root.jpg"));
    }

    #[test]
    fn overlapping_roots_do_not_add_the_same_file_twice() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("photos");
        fs::create_dir(&root).unwrap();
        let photo = root.join("photo.jpg");
        fs::write(&photo, b"image").unwrap();

        let result = scan(&[root, photo], true);

        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.skipped, 1);
    }
}
