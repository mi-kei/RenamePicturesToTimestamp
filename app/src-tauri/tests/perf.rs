use std::{fs, sync::atomic::AtomicBool, time::Instant};

use rename_photos_to_timestamp_gui_lib::{
    journal::JobKind,
    renamer::{self, RenameRequest},
};
use tempfile::tempdir;

const FILE_COUNT: usize = 20_000;

/// Run with `cargo test --release --test perf -- --ignored --nocapture`.
#[test]
#[ignore]
fn renames_twenty_thousand_files() {
    let root = tempdir().unwrap();
    let photos = root.path().join("photos");
    fs::create_dir_all(&photos).unwrap();

    let requests = (0..FILE_COUNT)
        .map(|index| {
            let from = photos.join(format!("DSC{index:05}.ARW"));
            fs::write(&from, b"x").unwrap();
            RenameRequest {
                id: Some(index as u32),
                to: photos.join(format!("IMG_20230223_125312_DSC{index:05}.ARW")),
                from,
            }
        })
        .collect();

    let started = Instant::now();
    let job = renamer::new_job(
        &root.path().join("journals"),
        JobKind::Rename,
        "",
        None,
        requests,
    )
    .unwrap();
    let prepared = started.elapsed();
    let report = job.run(&AtomicBool::new(false), |_| {});
    let total = started.elapsed();

    assert_eq!(report.ok as usize, FILE_COUNT);
    println!(
        "{FILE_COUNT} files: journal {prepared:?}, total {total:?} ({:.0} files/s)",
        FILE_COUNT as f64 / total.as_secs_f64()
    );
}
