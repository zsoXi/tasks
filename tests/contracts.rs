use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
use tasks::{Options, parse_input, run};
use tasks_core::{Timestamp, messages};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn cli(dir: &std::path::Path, arg: &str) -> Result<std::process::Output, std::io::Error> {
    Command::new(env!("CARGO_BIN_EXE_tasks"))
        .current_dir(dir)
        .arg(arg)
        .output()
}
#[test]
fn cli_stdin_arguments_errors_and_catalogue() -> TestResult {
    let dir = tempfile::tempdir()?;
    let create = cli(
        dir.path(),
        r#"{"op":"create","output":"list.json","title":"CLI"}"#,
    )?;
    assert!(create.status.success());
    let request = r#"{"op":"read","input":"list.json"}"#;
    let by_arg = cli(dir.path(), request)?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_tasks"))
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("stdin")?
        .write_all(request.as_bytes())?;
    assert_eq!(child.wait_with_output()?.stdout, by_arg.stdout);
    for (arg, code, field) in [
        ("broken", "tasks.invalid_json", None),
        (r#"{"op":"other"}"#, "tasks.enum", Some("/op")),
        (
            r#"{"op":"read","input":"list.json","x/y~":true}"#,
            "tasks.validation",
            Some("/x~1y~0"),
        ),
        (
            r#"{"op":"move","input":"list.json","expected_revision":0,"id":"t1","group":"unknown","index":0}"#,
            "tasks.enum",
            Some("/group"),
        ),
        (
            r#"{"op":"read","input":false}"#,
            "tasks.validation",
            Some("/input"),
        ),
    ] {
        let out = cli(dir.path(), arg)?;
        assert!(!out.status.success());
        assert!(out.stderr.is_empty());
        let v: Value = serde_json::from_slice(&out.stdout)?;
        assert_eq!(v["summary_code"], code);
        assert_eq!(v["warnings"], json!([]));
        assert_eq!(v["ok"], false);
        if let Some(field) = field {
            assert_eq!(v["summary_params"]["field"], field);
        }
        if code == "tasks.enum" {
            assert!(v["summary_params"]["allowed"].is_array());
        }
    }
    let multi = Command::new(env!("CARGO_BIN_EXE_tasks"))
        .args(["{}", "{}"])
        .output()?;
    assert!(!multi.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&multi.stdout)?["summary_code"],
        "tasks.arguments"
    );
    use std::os::unix::ffi::OsStringExt;
    let nonutf = Command::new(env!("CARGO_BIN_EXE_tasks"))
        .arg(std::ffi::OsString::from_vec(vec![255]))
        .output()?;
    assert!(!nonutf.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&nonutf.stdout)?["summary_code"],
        "tasks.invalid_utf8"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&cli(dir.path(), "--messages")?.stdout)?,
        messages::catalogue()
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&cli(dir.path(), "--schema")?.stdout)?,
        tasks::schema()
    );
    Ok(())
}
fn walk_schema(v: &Value) {
    if let Some(properties) = v["properties"].as_object() {
        for value in properties.values() {
            assert!(value["title"].is_string());
            assert!(value["description"].is_string());
            walk_schema(value);
        }
    }
    if let Some(items) = v.get("items") {
        walk_schema(items);
    }
    for key in v.as_object().into_iter().flat_map(|m| m.keys()) {
        assert!(
            [
                "type",
                "enum",
                "const",
                "default",
                "title",
                "description",
                "properties",
                "required",
                "items",
                "additionalProperties",
                "format",
                "pattern",
                "minimum",
                "maximum",
                "minLength",
                "maxLength",
                "minItems",
                "maxItems",
                "x-file",
                "x-accept",
                "x-formats"
            ]
            .contains(&key.as_str()),
            "{key}"
        );
    }
}
#[test]
fn schema_is_recursive_plain_forms_and_messages_complete() -> TestResult {
    let schema = tasks::schema();
    for form in schema["oneOf"].as_array().ok_or("forms")? {
        walk_schema(form);
    }
    let catalogue = messages::catalogue();
    let mut codes = std::collections::HashSet::new();
    for message in catalogue["messages"].as_array().ok_or("messages")? {
        assert!(codes.insert(message["code"].as_str().ok_or("code")?));
        let template = message["template"].as_str().ok_or("template")?;
        let params = message["params"].as_array().ok_or("params")?;
        for tail in template.split('{').skip(1) {
            let key = tail.split_once('}').ok_or("placeholder")?.0;
            assert!(params.contains(&json!(key)));
        }
        for param in params {
            assert!(template.contains(&format!("{{{}}}", param.as_str().ok_or("param")?)));
        }
    }
    Ok(())
}
#[test]
fn hostile_files_and_paths_are_coded_and_unchanged() -> TestResult {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let opts = Options {
        base_dir: dir.path().into(),
        now: Some(Timestamp::new("2026-10-15T18:00:00Z")?),
        ..Options::default()
    };
    let create = json!({"op":"create","output":"safe.json","title":"Safe"});
    run(&parse_input(&create.to_string())?, &opts)?;
    let before = fs::read(dir.path().join("safe.json"))?;
    for path in [
        "../escape.json",
        "/tmp/escape.json",
        "sub/../../escape.json",
    ] {
        let input = parse_input(&json!({"op":"create","output":path,"title":"Bad"}).to_string())?;
        let result = run(&input, &opts);
        assert!(result.is_err(), "{path}");
        if let Err(e) = result {
            assert_eq!(e.0.code.code(), "tasks.invalid_path");
        }
    }
    fs::write(outside.path().join("target.json"), &before)?;
    symlink(
        outside.path().join("target.json"),
        dir.path().join("link.json"),
    )?;
    symlink(
        outside.path().join("missing.json"),
        dir.path().join("dangling.json"),
    )?;
    symlink(outside.path(), dir.path().join("linked"))?;
    for path in ["link.json", "dangling.json", "linked/new.json"] {
        for op in ["create", "read"] {
            let request = if op == "create" {
                json!({"op":op,"output":path,"title":"Bad"})
            } else {
                json!({"op":op,"input":path})
            };
            let result = run(&parse_input(&request.to_string())?, &opts);
            assert!(result.is_err(), "{path}");
            if let Err(e) = result {
                assert_eq!(e.0.code.code(), "tasks.symlink");
            }
        }
    }
    fs::create_dir(dir.path().join("folder.json"))?;
    assert!(
        Command::new("mkfifo")
            .arg(dir.path().join("fifo.json"))
            .status()?
            .success()
    );
    for file in ["folder.json", "fifo.json"] {
        for request in [
            json!({"op":"read","input":file}),
            json!({"op":"create","output":file,"title":"Bad"}),
        ] {
            let mut child = Command::new(env!("CARGO_BIN_EXE_tasks"))
                .current_dir(dir.path())
                .arg(request.to_string())
                .stdout(Stdio::piped())
                .spawn()?;
            let start = std::time::Instant::now();
            loop {
                if child.try_wait()?.is_some() {
                    break;
                }
                if start.elapsed() > std::time::Duration::from_secs(2) {
                    child.kill()?;
                    return Err("nonregular input hung".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let result = child.wait_with_output()?;
            assert!(!result.status.success());
            assert_eq!(
                serde_json::from_slice::<Value>(&result.stdout)?["summary_code"],
                "tasks.not_regular_file"
            );
        }
    }
    let socket = std::os::unix::net::UnixListener::bind(dir.path().join("socket.json"))?;
    let result = cli(dir.path(), r#"{"op":"read","input":"socket.json"}"#)?;
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout)?["summary_code"],
        "tasks.not_regular_file"
    );
    drop(socket);
    fs::write(dir.path().join("bad.json"), "{")?;
    let error = cli(
        dir.path(),
        r#"{"op":"rename","input":"bad.json","expected_revision":0,"title":"Bad"}"#,
    )?;
    assert_eq!(
        serde_json::from_slice::<Value>(&error.stdout)?["summary_code"],
        "tasks.invalid_json"
    );
    assert_eq!(fs::read_to_string(dir.path().join("bad.json"))?, "{");
    fs::write(dir.path().join("utf.json"), [255])?;
    assert_eq!(
        serde_json::from_slice::<Value>(
            &cli(dir.path(), r#"{"op":"read","input":"utf.json"}"#)?.stdout
        )?["summary_code"],
        "tasks.invalid_utf8"
    );
    let small = Options {
        max_input_bytes: 2,
        ..opts.clone()
    };
    let failed = run(
        &parse_input(r#"{"op":"read","input":"safe.json"}"#)?,
        &small,
    );
    assert!(failed.is_err());
    assert_eq!(fs::read(dir.path().join("safe.json"))?, before);
    assert_eq!(fs::read(outside.path().join("target.json"))?, before);
    assert!(!outside.path().join("new.json").exists());
    Ok(())
}
#[test]
fn cross_process_determinism_and_revision_lock() -> TestResult {
    let dir = tempfile::tempdir()?;
    let opts = Options {
        base_dir: dir.path().into(),
        now: Some(Timestamp::new("2026-10-15T18:00:00Z")?),
        ..Options::default()
    };
    run(
        &parse_input(r#"{"op":"create","output":"list.json","title":"Stable"}"#)?,
        &opts,
    )?;
    let bytes = fs::read(dir.path().join("list.json"))?;
    let mut processes = vec![];
    for _ in 0..2 {
        processes.push(
            Command::new(env!("CARGO_BIN_EXE_tasks"))
                .current_dir(dir.path())
                .arg(
                    r#"{"op":"rename","input":"list.json","expected_revision":0,"title":"Stable"}"#,
                )
                .stdout(Stdio::piped())
                .spawn()?,
        );
    }
    let outcomes = processes
        .into_iter()
        .map(|p| p.wait_with_output())
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(outcomes.iter().filter(|o| o.status.success()).count(), 1);
    fs::write(dir.path().join("list.json"), bytes)?;
    let a = cli(
        dir.path(),
        r#"{"op":"snapshot","input":"list.json","output":"a.html"}"#,
    )?;
    let b = cli(
        dir.path(),
        r#"{"op":"snapshot","input":"list.json","output":"b.html"}"#,
    )?;
    assert!(a.status.success() && b.status.success());
    assert_eq!(a.stdout, b.stdout);
    assert_eq!(
        fs::read(dir.path().join("a.html"))?,
        fs::read(dir.path().join("b.html"))?
    );
    Ok(())
}
