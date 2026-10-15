use serde_json::{Value, json};
use std::{
    fs,
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
    },
};
use tasks::{Options, parse_input, run};
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn readers_never_observe_partial_writes_and_write_failure_preserves_bytes() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let opts = Options {
        base_dir: dir.path().into(),
        now: Some(tasks_core::Timestamp::new("2026-10-15T18:00:00Z")?),
        ..Options::default()
    };
    run(
        &parse_input(r#"{"op":"create","output":"list.json","title":"Original"}"#)?,
        &opts,
    )?;
    let mut raw: Value = serde_json::from_slice(&fs::read(dir.path().join("list.json"))?)?;
    raw["next_id"] = json!(501);
    raw["groups"][0]["tasks"]=json!((1..=500).map(|i|json!({"id":format!("t{i}"),"title":"Task","note":"A".repeat(1500),"created_at":"2026-10-15T18:00:00Z","updated_at":"2026-10-15T18:00:00Z","subtasks":[]})).collect::<Vec<_>>());
    fs::write(dir.path().join("list.json"), raw.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let barrier = Arc::new(Barrier::new(2));
    let path = dir.path().join("list.json");
    let done = stop.clone();
    let gate = barrier.clone();
    let reader = std::thread::spawn(move || -> Result<usize, String> {
        gate.wait();
        let mut reads = 0;
        while !done.load(Ordering::Acquire) {
            let bytes = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            tasks_core::parse(&bytes).map_err(|e| e.to_string())?;
            reads += 1;
        }
        Ok(reads)
    });
    barrier.wait();
    for revision in 0..20 {
        run(&parse_input(&json!({"op":"rename","input":"list.json","expected_revision":revision,"title":format!("Version {revision}")}).to_string())?,&opts)?;
    }
    stop.store(true, Ordering::Release);
    assert!(reader.join().map_err(|_| "reader crashed")?? > 0);
    let before = fs::read(dir.path().join("list.json"))?;
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500))?;
    let failure = run(
        &parse_input(
            r#"{"op":"rename","input":"list.json","expected_revision":20,"title":"Cannot save"}"#,
        )?,
        &opts,
    );
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
    assert!(failure.is_err());
    assert_eq!(fs::read(dir.path().join("list.json"))?, before);
    assert_eq!(fs::read_dir(dir.path())?.count(), 1);
    Ok(())
}
