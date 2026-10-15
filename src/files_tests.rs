use super::test_hooks;
use crate::{Options, parse_input, run};
use std::{
    fs,
    path::Path,
    sync::mpsc,
    time::{Duration, SystemTime},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn e2e_rename_failure_preserves_old_bytes() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    let options = opts(root.path())?;
    seed(&options)?;
    let before = fs::read(root.path().join("list.json"))?;
    let folder = root.path().to_owned();
    test_hooks::BEFORE_RENAME.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            assert!(fs::set_permissions(folder, fs::Permissions::from_mode(0o500)).is_ok());
        }))
    });
    let result = run(
        &parse_input(
            r#"{"op":"rename","input":"list.json","expected_revision":0,"title":"Must not persist"}"#,
        )?,
        &options,
    );
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))?;
    assert!(result.is_err());
    assert_eq!(fs::read(root.path().join("list.json"))?, before);
    Ok(())
}
#[test]
fn e2e_size_is_checked_before_open() -> TestResult {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let root = tempfile::tempdir()?;
    let mut options = opts(root.path())?;
    seed(&options)?;
    options.max_input_bytes = 2;
    let fired = Arc::new(AtomicBool::new(false));
    let signal = fired.clone();
    test_hooks::BEFORE_READ_OPEN.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            signal.store(true, Ordering::SeqCst);
        }))
    });
    let result = run(
        &parse_input(r#"{"op":"read","input":"list.json"}"#)?,
        &options,
    );
    test_hooks::BEFORE_READ_OPEN.with(|h| *h.borrow_mut() = None);
    assert_eq!(
        result.err().map(|e| e.0.code.code()),
        Some("tasks.input_too_large")
    );
    assert!(!fired.load(Ordering::SeqCst));
    Ok(())
}
fn opts(base: &Path) -> Result<Options, tasks_core::Error> {
    Ok(Options {
        base_dir: base.into(),
        now: Some(tasks_core::Timestamp::new("2026-10-15T18:00:00Z")?),
        ..Options::default()
    })
}
fn seed(opts: &Options) -> TestResult {
    run(
        &parse_input(r#"{"op":"create","output":"list.json","title":"Pinned"}"#)?,
        opts,
    )?;
    Ok(())
}
#[test]
fn e2e_directory_replacement_is_pinned() -> TestResult {
    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    fs::create_dir(root.path().join("folder"))?;
    let options = opts(root.path())?;
    run(
        &parse_input(r#"{"op":"create","output":"folder/list.json","title":"Original"}"#)?,
        &options,
    )?;
    fs::write(outside.path().join("list.json"), "outside")?;
    let r = root.path().to_owned();
    let o = outside.path().to_owned();
    test_hooks::TARGET_OPENED.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            assert!(fs::rename(r.join("folder"), r.join("pinned")).is_ok());
            assert!(std::os::unix::fs::symlink(o, r.join("folder")).is_ok());
        }))
    });
    run(
        &parse_input(
            r#"{"op":"rename","input":"folder/list.json","expected_revision":0,"title":"Changed"}"#,
        )?,
        &options,
    )?;
    let doc = tasks_core::parse(&fs::read_to_string(root.path().join("pinned/list.json"))?)?;
    assert_eq!(tasks_core::view(&doc)["list"]["title"], "Changed");
    assert_eq!(
        fs::read_to_string(outside.path().join("list.json"))?,
        "outside"
    );
    Ok(())
}
#[test]
fn e2e_leaf_swap_to_symlink_never_follows() -> TestResult {
    let root = tempfile::tempdir()?;
    let options = opts(root.path())?;
    seed(&options)?;
    let before = fs::read(root.path().join("list.json"))?;
    fs::write(root.path().join("other.json"), &before)?;
    let rootpath = root.path().to_owned();
    test_hooks::BEFORE_READ_OPEN.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            assert!(fs::remove_file(rootpath.join("list.json")).is_ok());
            assert!(std::os::unix::fs::symlink("other.json", rootpath.join("list.json")).is_ok());
        }))
    });
    let result = run(
        &parse_input(r#"{"op":"read","input":"list.json"}"#)?,
        &options,
    );
    assert_eq!(result.err().map(|e| e.0.code.code()), Some("tasks.symlink"));
    assert_eq!(fs::read(root.path().join("other.json"))?, before);
    Ok(())
}
#[test]
fn e2e_fifo_swap_nonblocking_and_descriptor_type_check() -> TestResult {
    let root = tempfile::tempdir()?;
    let options = opts(root.path())?;
    seed(&options)?;
    assert!(
        std::process::Command::new("mkfifo")
            .arg(root.path().join("pipe"))
            .status()?
            .success()
    );
    let path = root.path().to_owned();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        test_hooks::BEFORE_READ_OPEN.with(|h| {
            *h.borrow_mut() = Some(Box::new(move || {
                assert!(fs::rename(path.join("pipe"), path.join("list.json")).is_ok());
            }))
        });
        let result = parse_input(r#"{"op":"read","input":"list.json"}"#)
            .and_then(|input| run(&input, &options));
        let _ = tx.send(result.err().map(|e| e.0.code.code()));
    });
    let outcome = rx.recv_timeout(Duration::from_secs(2))?;
    worker.join().map_err(|_| "worker failed")?;
    assert_eq!(outcome, Some("tasks.not_regular_file"));
    Ok(())
}
#[test]
fn e2e_growing_file_rechecked_before_read() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut options = opts(root.path())?;
    seed(&options)?;
    options.max_input_bytes = 1024;
    let path = root.path().join("list.json");
    let grow = path.clone();
    test_hooks::READ_OPENED.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            assert!(fs::write(&grow, vec![b'x'; 2048]).is_ok());
            let file = fs::File::open(&grow);
            assert!(file.is_ok());
            if let Ok(file) = file {
                assert!(
                    file.set_times(fs::FileTimes::new().set_accessed(SystemTime::UNIX_EPOCH))
                        .is_ok()
                );
            }
        }))
    });
    let result = run(
        &parse_input(r#"{"op":"read","input":"list.json"}"#)?,
        &options,
    );
    assert_eq!(
        result.err().map(|e| e.0.code.code()),
        Some("tasks.input_too_large")
    );
    assert_eq!(fs::metadata(path)?.accessed()?, SystemTime::UNIX_EPOCH);
    Ok(())
}
