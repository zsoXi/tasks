use serde_json::{Value, json};
use std::{fs, process::Command};
use tasks_core::{Edit, Timestamp, apply, new_list, snapshot_html, to_json};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn deterministic_fixture() -> Result<(String, String), Box<dyn std::error::Error>> {
    let now = Timestamp::new("2026-10-15T18:00:00Z")?;
    let mut doc = new_list("stable-list", "<Launch> 😀", now.clone())?;
    for mut v in [
        json!({"op":"add","group":"now","title":"Send","note":"private"}),
        json!({"op":"add","parent":"t1","title":"Check"}),
        json!({"op":"check","id":"t2","done":true}),
        json!({"op":"outdent","id":"t2"}),
        json!({"op":"move","id":"t2","group":"done","index":0}),
    ] {
        v["expected_revision"] = json!(doc.revision());
        let edit: Edit = serde_json::from_value(v)?;
        doc = apply(&doc, &edit, now.clone())?;
    }
    Ok((to_json(&doc), snapshot_html(&doc, &Default::default()).html))
}
#[test]
fn deterministic_child() -> TestResult {
    if let Some(path) = std::env::var_os("TASKS_TEST_ARTIFACT") {
        let (json, html) = deterministic_fixture()?;
        fs::write(std::path::Path::new(&path).with_extension("json"), json)?;
        fs::write(std::path::Path::new(&path).with_extension("html"), html)?;
    }
    Ok(())
}
#[test]
fn i2_same_edits_time_across_processes() -> TestResult {
    let a = deterministic_fixture()?;
    let b = deterministic_fixture()?;
    assert_eq!(a, b);
    let dir = tempfile::tempdir()?;
    for name in ["a", "b"] {
        let status = Command::new(std::env::current_exe()?)
            .args(["--exact", "deterministic_child"])
            .env("TASKS_TEST_ARTIFACT", dir.path().join(name))
            .status()?;
        assert!(status.success());
        assert_eq!(
            fs::read_to_string(dir.path().join(format!("{name}.json")))?,
            a.0
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(format!("{name}.html")))?,
            a.1
        );
    }
    let parsed: Value = serde_json::from_str(&a.0)?;
    assert_eq!(parsed["revision"], 5);
    Ok(())
}
