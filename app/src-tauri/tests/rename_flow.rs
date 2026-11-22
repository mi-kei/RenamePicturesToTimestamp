use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use rename_photos_to_timestamp_gui_lib::{
    journal::{self, JobKind},
    renamer::{self, RenameRequest},
};
use tempfile::tempdir;

struct Fixture {
    _root: tempfile::TempDir,
    photos: PathBuf,
    journals: PathBuf,
}

fn fixture() -> Fixture {
    let root = tempdir().unwrap();
    let photos = root.path().join("photos");
    let journals = root.path().join("journals");
    fs::create_dir_all(&photos).unwrap();
    Fixture {
        _root: root,
        photos,
        journals,
    }
}

fn write(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, name.as_bytes()).unwrap();
    path
}

fn request(dir: &Path, from: &str, to: &str) -> RenameRequest {
    RenameRequest {
        id: None,
        from: dir.join(from),
        to: dir.join(to),
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn content(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name)).unwrap()
}

fn run_all(job: renamer::Job) -> renamer::JobReport {
    job.run(&AtomicBool::new(false), |_| {})
}

#[test]
fn renames_everything_and_writes_a_complete_journal() {
    let f = fixture();
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        write(&f.photos, name);
    }
    let requests = ["a", "b", "c"]
        .iter()
        .map(|stem| {
            request(
                &f.photos,
                &format!("{stem}.jpg"),
                &format!("IMG_{stem}.jpg"),
            )
        })
        .collect();

    let job = renamer::new_job(&f.journals, JobKind::Rename, "IMG_{name}", None, requests).unwrap();
    let job_id = job.id().to_owned();
    let report = run_all(job);

    assert_eq!(
        (report.ok, report.failed, report.state.as_str()),
        (3, 0, "completed")
    );
    assert_eq!(names(&f.photos), ["IMG_a.jpg", "IMG_b.jpg", "IMG_c.jpg"]);
    assert_eq!(content(&f.photos, "IMG_a.jpg"), "a.jpg");

    let text = fs::read_to_string(journal::journal_path(&f.journals, &job_id)).unwrap();
    for record in [
        "\"header\"",
        "\"intent\"",
        "\"done\"",
        "\"footer\",\"state\":\"completed\"",
    ] {
        assert!(text.contains(record), "journal is missing {record}");
    }
}

#[test]
fn chained_and_swapped_renames_use_temporary_names() {
    let f = fixture();
    for name in ["a.jpg", "b.jpg", "x.jpg", "y.jpg"] {
        write(&f.photos, name);
    }
    let requests = vec![
        request(&f.photos, "a.jpg", "b.jpg"),
        request(&f.photos, "b.jpg", "c.jpg"),
        request(&f.photos, "x.jpg", "y.jpg"),
        request(&f.photos, "y.jpg", "x.jpg"),
    ];

    let job = renamer::new_job(&f.journals, JobKind::Rename, "", None, requests).unwrap();
    let report = run_all(job);

    assert_eq!((report.ok, report.failed), (4, 0));
    assert_eq!(names(&f.photos), ["b.jpg", "c.jpg", "x.jpg", "y.jpg"]);
    assert_eq!(content(&f.photos, "b.jpg"), "a.jpg");
    assert_eq!(content(&f.photos, "c.jpg"), "b.jpg");
    assert_eq!(content(&f.photos, "x.jpg"), "y.jpg");
    assert_eq!(content(&f.photos, "y.jpg"), "x.jpg");
}

#[test]
fn undo_restores_every_original_name() {
    let f = fixture();
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        write(&f.photos, name);
    }
    let requests = vec![
        request(&f.photos, "a.jpg", "b.jpg"),
        request(&f.photos, "b.jpg", "c.jpg"),
        request(&f.photos, "c.jpg", "IMG_c.jpg"),
    ];
    let job = renamer::new_job(&f.journals, JobKind::Rename, "", None, requests).unwrap();
    let job_id = job.id().to_owned();
    run_all(job);

    let undo = renamer::undo(&f.journals, &job_id).unwrap();
    assert_eq!(undo.kind(), JobKind::Undo);
    let report = run_all(undo);

    assert_eq!((report.ok, report.failed), (3, 0));
    assert_eq!(names(&f.photos), ["a.jpg", "b.jpg", "c.jpg"]);
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        assert_eq!(content(&f.photos, name), name);
    }
    let infos = journal::list(&f.journals);
    let original = infos.iter().find(|info| info.job == job_id).unwrap();
    assert!(original.undone_by.is_some());
    assert!(renamer::undo(&f.journals, &job_id).is_err());
}

#[test]
fn cancelled_and_interrupted_jobs_can_be_resumed() {
    let f = fixture();
    let requests = (1..=5)
        .map(|number| {
            write(&f.photos, &format!("{number}.jpg"));
            request(
                &f.photos,
                &format!("{number}.jpg"),
                &format!("IMG_{number}.jpg"),
            )
        })
        .collect();
    let job = renamer::new_job(&f.journals, JobKind::Rename, "", None, requests).unwrap();
    let job_id = job.id().to_owned();
    let cancel = AtomicBool::new(false);

    let report = job.run(&cancel, |item| {
        if item.done == 2 {
            cancel.store(true, Ordering::SeqCst);
        }
    });

    assert_eq!((report.ok, report.state.as_str()), (2, "cancelled"));
    let path = journal::journal_path(&f.journals, &job_id);
    assert!(fs::read_to_string(&path)
        .unwrap()
        .trim_end()
        .ends_with(r#"{"type":"footer","state":"cancelled"}"#));

    // Simulate a crash by removing the footer: the job becomes incomplete.
    let text = fs::read_to_string(&path).unwrap();
    let without_footer = text.trim_end().rsplit_once('\n').unwrap().0.to_owned() + "\n";
    fs::write(&path, without_footer).unwrap();
    assert!(journal::list(&f.journals)[0].incomplete);

    let resumed = renamer::resume(&f.journals, &job_id).unwrap();
    assert_eq!(resumed.total(), 3);
    let report = run_all(resumed);

    assert_eq!(
        (report.ok, report.failed, report.state.as_str()),
        (3, 0, "completed")
    );
    assert_eq!(
        names(&f.photos),
        [
            "IMG_1.jpg",
            "IMG_2.jpg",
            "IMG_3.jpg",
            "IMG_4.jpg",
            "IMG_5.jpg"
        ]
    );
    assert!(!journal::list(&f.journals)[0].incomplete);
}

#[test]
fn a_target_created_after_planning_is_never_overwritten() {
    let f = fixture();
    write(&f.photos, "a.jpg");
    let job = renamer::new_job(
        &f.journals,
        JobKind::Rename,
        "",
        None,
        vec![request(&f.photos, "a.jpg", "IMG_a.jpg")],
    )
    .unwrap();
    fs::write(f.photos.join("IMG_a.jpg"), b"someone else").unwrap();

    let mut errors = Vec::new();
    let report = job.run(&AtomicBool::new(false), |item| {
        if let Err(error) = &item.result {
            errors.push(error.clone());
        }
    });

    assert_eq!((report.ok, report.failed), (0, 1));
    assert_eq!(errors.len(), 1);
    assert_eq!(content(&f.photos, "a.jpg"), "a.jpg");
    assert_eq!(content(&f.photos, "IMG_a.jpg"), "someone else");
}
