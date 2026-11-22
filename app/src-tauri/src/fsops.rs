use std::{io, path::Path};

/// Renames `from` to `to` without ever replacing an existing file at `to`.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    if from == to {
        return Ok(());
    }
    if differs_only_by_case(from, to) {
        return rename_case_only(from, to);
    }
    platform_rename(from, to)
}

fn differs_only_by_case(from: &Path, to: &Path) -> bool {
    from.parent() == to.parent()
        && match (from.file_name(), to.file_name()) {
            (Some(from), Some(to)) => {
                from.to_string_lossy().to_lowercase() == to.to_string_lossy().to_lowercase()
            }
            _ => false,
        }
}

#[cfg(windows)]
fn rename_case_only(from: &Path, to: &Path) -> io::Result<()> {
    platform_rename(from, to)
}

#[cfg(not(windows))]
fn rename_case_only(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    match (from.symlink_metadata(), to.symlink_metadata()) {
        (Ok(source), Ok(target))
            if source.dev() == target.dev() && source.ino() == target.ino() =>
        {
            std::fs::rename(from, to)
        }
        (_, Ok(_)) => Err(already_exists(to)),
        _ => platform_rename(from, to),
    }
}

#[cfg(windows)]
fn platform_rename(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let from = wide(from);
    let to = wide(to);
    // Without MOVEFILE_REPLACE_EXISTING the call fails when the target exists.
    let succeeded = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn platform_rename(from: &Path, to: &Path) -> io::Result<()> {
    let from_c = c_path(from)?;
    let to_c = c_path(to)?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from_c.as_ptr(),
            libc::AT_FDCWD,
            to_c.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EINVAL) | Some(libc::ENOSYS) | Some(libc::EOPNOTSUPP) => {
            checked_rename(from, to)
        }
        _ => Err(error),
    }
}

#[cfg(target_os = "macos")]
fn platform_rename(from: &Path, to: &Path) -> io::Result<()> {
    let from_c = c_path(from)?;
    let to_c = c_path(to)?;
    let result = unsafe { libc::renamex_np(from_c.as_ptr(), to_c.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EINVAL) | Some(libc::ENOTSUP) => checked_rename(from, to),
        _ => Err(error),
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn platform_rename(from: &Path, to: &Path) -> io::Result<()> {
    checked_rename(from, to)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn c_path(path: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

/// Fallback for file systems without an atomic no-replace rename.
#[cfg(unix)]
fn checked_rename(from: &Path, to: &Path) -> io::Result<()> {
    if to.symlink_metadata().is_ok() {
        return Err(already_exists(to));
    }
    std::fs::rename(from, to)
}

#[cfg(unix)]
fn already_exists(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{} already exists", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, io::ErrorKind};

    use tempfile::tempdir;

    use super::rename_no_replace;

    #[test]
    fn renames_when_the_target_is_free() {
        let directory = tempdir().unwrap();
        let from = directory.path().join("a.jpg");
        let to = directory.path().join("b.jpg");
        fs::write(&from, b"a").unwrap();

        rename_no_replace(&from, &to).unwrap();

        assert!(!from.exists());
        assert_eq!(fs::read(&to).unwrap(), b"a");
    }

    #[test]
    fn never_replaces_an_existing_target() {
        let directory = tempdir().unwrap();
        let from = directory.path().join("a.jpg");
        let to = directory.path().join("b.jpg");
        fs::write(&from, b"a").unwrap();
        fs::write(&to, b"b").unwrap();

        let error = rename_no_replace(&from, &to).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&from).unwrap(), b"a");
        assert_eq!(fs::read(&to).unwrap(), b"b");
    }

    #[test]
    fn supports_case_only_renames() {
        let directory = tempdir().unwrap();
        let from = directory.path().join("photo.jpg");
        let to = directory.path().join("PHOTO.JPG");
        fs::write(&from, b"a").unwrap();

        rename_no_replace(&from, &to).unwrap();

        let names = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, ["PHOTO.JPG"]);
    }
}
