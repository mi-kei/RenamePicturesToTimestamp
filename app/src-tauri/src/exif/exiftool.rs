use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use serde_json::Value;

use crate::scan::path_key;

use super::{parse_datetime, ExifInfo, ExifSource};

const READY_TIMEOUT: Duration = Duration::from_secs(120);

pub struct ExifToolProcess {
    child: Child,
    stdin: ChildStdin,
    output: Receiver<Result<String, String>>,
}

impl ExifToolProcess {
    pub fn start() -> Result<Self, String> {
        let mut command = Command::new("exiftool");
        command
            .args(["-stay_open", "True", "-@", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().map_err(|error| error.to_string())?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to open ExifTool stdin".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to open ExifTool stdout".to_owned())?;
        let (sender, output) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let message = line.map_err(|error| error.to_string());
                let failed = message.is_err();
                if sender.send(message).is_err() || failed {
                    break;
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            output,
        })
    }

    pub fn read_batch(&mut self, paths: &[PathBuf]) -> Result<HashMap<String, ExifInfo>, String> {
        let mut arguments = vec![
            "-charset".to_owned(),
            "filename=utf8".to_owned(),
            "-json".to_owned(),
            "-DateTimeOriginal".to_owned(),
            "-DateTimeDigitized".to_owned(),
            "-CreateDate".to_owned(),
            "-Make".to_owned(),
            "-Model".to_owned(),
        ];

        for path in paths {
            let path = path.to_string_lossy();
            if path.contains('\n') || path.contains('\r') {
                return Err(format!(
                    "ExifTool cannot process a path containing a newline: {path}"
                ));
            }
            arguments.push(path.into_owned());
        }

        for argument in arguments {
            writeln!(self.stdin, "{argument}").map_err(|error| error.to_string())?;
        }
        writeln!(self.stdin, "-execute").map_err(|error| error.to_string())?;
        self.stdin.flush().map_err(|error| error.to_string())?;

        let mut json = String::new();
        loop {
            let line = self
                .output
                .recv_timeout(READY_TIMEOUT)
                .map_err(|error| format!("ExifTool did not finish the request: {error}"))??;
            if line.trim().starts_with("{ready") {
                break;
            }
            json.push_str(&line);
            json.push('\n');
        }

        let records: Vec<Value> = serde_json::from_str(&json)
            .map_err(|error| format!("ExifTool returned invalid JSON: {error}"))?;
        Ok(records
            .into_iter()
            .filter_map(parse_record)
            .collect::<HashMap<_, _>>())
    }
}

impl Drop for ExifToolProcess {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"-stay_open\nFalse\n");
        let _ = self.stdin.flush();
        let _ = self.child.wait();
    }
}

fn parse_record(record: Value) -> Option<(String, ExifInfo)> {
    let source_file = record.get("SourceFile")?.as_str()?.to_owned();
    let taken_at = ["DateTimeOriginal", "DateTimeDigitized", "CreateDate"]
        .into_iter()
        .filter_map(|tag| record.get(tag).and_then(value_as_string))
        .find_map(|value| parse_datetime(&value));

    Some((
        path_key(Path::new(&source_file)),
        ExifInfo {
            taken_at,
            source: ExifSource::ExifTool,
            make: record.get("Make").and_then(value_as_string),
            model: record.get("Model").and_then(value_as_string),
        },
    ))
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Array(values) => values.first().and_then(value_as_string),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_record;
    use crate::exif::ExifSource;

    #[test]
    fn parses_exiftool_json_and_prefers_original_date() {
        let (path, info) = parse_record(json!({
            "SourceFile": "C:/photos/one.ARW",
            "DateTimeOriginal": "2023:02:23 12:53:12",
            "DateTimeDigitized": "2020:01:01 00:00:00",
            "CreateDate": "2019:01:01 00:00:00",
            "Make": "Sony",
            "Model": "ILCE-7M4"
        }))
        .unwrap();

        assert!(path.ends_with("photos/one.arw"));
        assert_eq!(
            info.taken_at
                .unwrap()
                .format("%Y:%m:%d %H:%M:%S")
                .to_string(),
            "2023:02:23 12:53:12"
        );
        assert_eq!(info.make.as_deref(), Some("Sony"));
        assert_eq!(info.model.as_deref(), Some("ILCE-7M4"));
        assert!(matches!(info.source, ExifSource::ExifTool));
    }
}
