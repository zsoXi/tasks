use serde_json::json;
use tasks::{Options, parse_input, run};
#[test]
fn absolute_and_parent_paths_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let base = sandbox.path().join("base");
    std::fs::create_dir(&base)?;
    let opts = Options {
        base_dir: base,
        now: Some(tasks_core::Timestamp::new("2026-10-15T18:00:00Z")?),
        ..Options::default()
    };
    for path in [
        sandbox
            .path()
            .join("absolute.json")
            .to_str()
            .ok_or("utf8")?
            .to_owned(),
        "../escape.json".into(),
    ] {
        let request =
            parse_input(&json!({"op":"create","output":path,"title":"Invalid"}).to_string())?;
        let result = run(&request, &opts);
        assert!(result.is_err(), "accepted {path}");
        if let Err(e) = result {
            assert_eq!(e.0.code.code(), "tasks.invalid_path");
        }
    }
    assert!(!sandbox.path().join("escape.json").exists());
    assert!(!sandbox.path().join("absolute.json").exists());
    Ok(())
}
#[test]
fn parent_traversal_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let base = sandbox.path().join("base");
    std::fs::create_dir(&base)?;
    let opts = Options {
        base_dir: base,
        ..Options::default()
    };
    let input = parse_input(r#"{"op":"create","output":"../escape.json","title":"Invalid"}"#)?;
    assert!(run(&input, &opts).is_err());
    assert!(!sandbox.path().join("escape.json").exists());
    Ok(())
}
