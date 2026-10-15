use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
use tasks::{Options, parse_input, run};
use tasks_core::{Timestamp, messages};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn options(dir: &std::path::Path) -> Result<Options, tasks_core::Error> {
    Ok(Options {
        base_dir: dir.into(),
        now: Some(Timestamp::new("2026-10-15T18:00:00Z")?),
        ..Options::default()
    })
}
fn call(opts: &Options, v: Value) -> Result<Value, tasks::Error> {
    Ok(json!(run(&parse_input(&v.to_string())?, opts)?))
}
fn create(opts: &Options) -> Result<(), tasks::Error> {
    call(
        opts,
        json!({"op":"create","output":"list.json","title":"Launch"}),
    )?;
    Ok(())
}
fn edit(opts: &Options, mut v: Value) -> Result<Value, tasks::Error> {
    v["input"] = json!("list.json");
    let doc = tasks_core::parse(
        &fs::read_to_string(opts.base_dir.join("list.json"))
            .map_err(|e| tasks::Error(tasks_core::Error::caused(messages::MessageCode::Io, e)))?,
    )?;
    v["expected_revision"] = json!(doc.revision());
    call(opts, v)
}
fn failure(opts: &Options, v: Value, code: &str) -> TestResult {
    let before = fs::read(opts.base_dir.join("list.json"))?;
    let result = edit(opts, v);
    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.0.code.code(), code);
        assert_eq!(json!(e.response("en"))["warnings"], json!([]));
    }
    assert_eq!(fs::read(opts.base_dir.join("list.json"))?, before);
    Ok(())
}
#[test]
fn s1_exact_read_shape_and_all_edits() -> TestResult {
    let dir = tempfile::tempdir()?;
    let opts = options(dir.path())?;
    create(&opts)?;
    for title in ["Send invoice", "Call", "Ship"] {
        edit(&opts, json!({"op":"add","group":"now","title":title}))?;
    }
    edit(&opts, json!({"op":"add","group":"ideas","title":"Idea"}))?;
    for title in ["Check VAT", "Attach PDF"] {
        edit(&opts, json!({"op":"add","parent":"t1","title":title}))?;
    }
    edit(&opts, json!({"op":"check","id":"t5","done":true}))?;
    let result = call(&opts, json!({"op":"read","input":"list.json"}))?;
    assert_eq!(result["warnings"], json!([]));
    assert_eq!(
        result["data"],
        json!({"list":{"id":"l1","title":"Launch","revision":7},"groups":[
        {"key":"now","tasks":[{"id":"t1","title":"Send invoice","subtasks":[{"id":"t5","title":"Check VAT","done":true},{"id":"t6","title":"Attach PDF","done":false}],"progress":{"done":1,"total":2}}, {"id":"t2","title":"Call","subtasks":[]},{"id":"t3","title":"Ship","subtasks":[]}]},
        {"key":"later","tasks":[]},{"key":"ongoing","tasks":[]},{"key":"ideas","tasks":[{"id":"t4","title":"Idea","subtasks":[]}]},{"key":"done","tasks":[]},{"key":"rejected","tasks":[]}]})
    );
    let checked = edit(&opts, json!({"op":"check","id":"t5","done":true}))?;
    assert_eq!(
        checked["data"]["groups"][0]["tasks"][0]["progress"],
        json!({"done":1,"total":2})
    );
    edit(
        &opts,
        json!({"op":"edit","id":"t1","title":" Invoice ","note":"Details\nHere"}),
    )?;
    edit(&opts, json!({"op":"edit","id":"t5","title":"VAT"}))?;
    let cleared = edit(&opts, json!({"op":"edit","id":"t1","note":null}))?;
    assert!(
        cleared["data"]["groups"][0]["tasks"][0]
            .get("note")
            .is_none()
    );
    edit(&opts, json!({"op":"rename","title":"New name"}))?;
    edit(&opts, json!({"op":"delete","id":"t5"}))?;
    let deleted = edit(&opts, json!({"op":"delete","id":"t1"}))?;
    assert_eq!(
        deleted["data"]["groups"][0]["tasks"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    failure(&opts, json!({"op":"delete","id":"t6"}), "tasks.not_found")?;
    Ok(())
}
#[test]
fn s2_indent_outdent_and_every_error() -> TestResult {
    let dir = tempfile::tempdir()?;
    let opts = options(dir.path())?;
    create(&opts)?;
    for t in ["Parent", "Child", "Third"] {
        edit(&opts, json!({"op":"add","group":"now","title":t}))?;
    }
    failure(
        &opts,
        json!({"op":"indent","id":"t1"}),
        "tasks.no_task_above",
    )?;
    edit(&opts, json!({"op":"edit","id":"t2","note":"Keep me"}))?;
    failure(&opts, json!({"op":"indent","id":"t2"}), "tasks.has_note")?;
    edit(&opts, json!({"op":"edit","id":"t2","note":null}))?;
    edit(&opts, json!({"op":"add","parent":"t2","title":"Step"}))?;
    failure(
        &opts,
        json!({"op":"indent","id":"t2"}),
        "tasks.has_subtasks",
    )?;
    edit(&opts, json!({"op":"delete","id":"t4"}))?;
    let indented = edit(&opts, json!({"op":"indent","id":"t2"}))?;
    assert_eq!(
        indented["data"]["groups"][0]["tasks"][0]["subtasks"],
        json!([{"id":"t2","title":"Child","done":false}])
    );
    failure(
        &opts,
        json!({"op":"edit","id":"t2","note":null}),
        "tasks.subtask_note",
    )?;
    failure(
        &opts,
        json!({"op":"add","parent":"t1","title":"Bad","note":"No"}),
        "tasks.subtask_note",
    )?;
    for op in ["check", "outdent"] {
        failure(
            &opts,
            json!({"op":op,"id":"t1","done":true})
                .as_object()
                .map(|m| {
                    let mut m = m.clone();
                    if op != "check" {
                        m.remove("done");
                    }
                    Value::Object(m)
                })
                .ok_or("object")?,
            "tasks.not_subtask",
        )?;
    }
    failure(&opts, json!({"op":"indent","id":"t2"}), "tasks.not_subtask")?;
    failure(
        &opts,
        json!({"op":"move","id":"t2","group":"done","index":0}),
        "tasks.not_subtask",
    )?;
    failure(
        &opts,
        json!({"op":"add","parent":"t2","title":"Deep"}),
        "tasks.not_subtask",
    )?;
    edit(&opts, json!({"op":"check","id":"t2","done":true}))?;
    let promoted = edit(&opts, json!({"op":"outdent","id":"t2"}))?;
    assert_eq!(promoted["warnings"][0]["code"], "tasks.done_flag_dropped");
    assert_eq!(
        promoted["data"]["groups"][0]["tasks"][1],
        json!({"id":"t2","title":"Child","subtasks":[]})
    );
    let again = edit(&opts, json!({"op":"indent","id":"t2"}))?;
    assert_eq!(
        again["data"]["groups"][0]["tasks"][0]["subtasks"][0]["done"],
        false
    );
    let again = edit(&opts, json!({"op":"outdent","id":"t2"}))?;
    assert_eq!(again["warnings"], json!([]));
    for _ in 0..50 {
        edit(&opts, json!({"op":"add","parent":"t1","title":"Step"}))?;
    }
    failure(
        &opts,
        json!({"op":"indent","id":"t2"}),
        "tasks.too_many_subtasks",
    )?;
    failure(
        &opts,
        json!({"op":"add","parent":"t1","title":"Step"}),
        "tasks.too_many_subtasks",
    )?;
    for op in ["indent", "outdent", "edit", "delete", "check", "move"] {
        let mut v = json!({"op":op,"id":"missing"});
        if op == "check" {
            v["done"] = json!(true);
        }
        if op == "move" {
            v["group"] = json!("now");
            v["index"] = json!(0);
        }
        failure(&opts, v, "tasks.not_found")?;
    }
    Ok(())
}
#[test]
fn s3_move_exact_positions() -> TestResult {
    let dir = tempfile::tempdir()?;
    let opts = options(dir.path())?;
    create(&opts)?;
    edit(&opts, json!({"op":"add","group":"now","title":"Finish"}))?;
    for title in ["A", "B", "C"] {
        edit(&opts, json!({"op":"add","group":"later","title":title}))?;
    }
    edit(
        &opts,
        json!({"op":"move","id":"t1","group":"done","index":0}),
    )?;
    let moved = edit(
        &opts,
        json!({"op":"move","id":"t2","group":"later","index":2}),
    )?;
    assert_eq!(
        moved["data"]["groups"][1]["tasks"]
            .as_array()
            .ok_or("tasks")?
            .iter()
            .map(|t| t["id"].clone())
            .collect::<Vec<_>>(),
        vec![json!("t3"), json!("t4"), json!("t2")]
    );
    assert_eq!(moved["data"]["groups"][4]["tasks"][0]["id"], "t1");
    failure(
        &opts,
        json!({"op":"move","id":"t2","group":"later","index":3}),
        "tasks.index_out_of_range",
    )?;
    failure(
        &opts,
        json!({"op":"add","group":"now","title":"X","index":1}),
        "tasks.index_out_of_range",
    )?;
    failure(
        &opts,
        json!({"op":"add","parent":"t2","title":"X","index":1}),
        "tasks.index_out_of_range",
    )?;
    Ok(())
}
#[test]
fn s4_stale_and_parallel_writers() -> TestResult {
    let dir = tempfile::tempdir()?;
    let opts = options(dir.path())?;
    create(&opts)?;
    edit(&opts, json!({"op":"add","group":"now","title":"A"}))?;
    let before = fs::read(dir.path().join("list.json"))?;
    let stale = call(
        &opts,
        json!({"op":"rename","input":"list.json","title":"Bad","expected_revision":0}),
    );
    assert!(stale.is_err());
    if let Err(e) = stale {
        assert_eq!(e.0.code.code(), "tasks.conflict");
        assert_eq!(e.0.params, json!({"expected":0,"actual":1}));
    }
    assert_eq!(fs::read(dir.path().join("list.json"))?, before);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = vec![];
    for title in ["B", "C"] {
        let opts = opts.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || { barrier.wait(); call(&opts,json!({"op":"add","input":"list.json","expected_revision":1,"group":"now","title":title})) }));
    }
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|w| w.join().map_err(|_| "worker failed"))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| r
                .as_ref()
                .is_err_and(|e| e.0.code.code() == "tasks.conflict"))
            .count(),
        1
    );
    assert_eq!(
        tasks_core::parse(&fs::read_to_string(dir.path().join("list.json"))?)?.revision(),
        2
    );
    edit(&opts, json!({"op":"add","group":"now","title":"D"}))?;
    edit(&opts, json!({"op":"add","group":"now","title":"E"}))?;
    Ok(())
}
#[test]
fn s5_snapshot_safe_and_hash() -> TestResult {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir()?;
    let opts = options(dir.path())?;
    create(&opts)?;
    edit(
        &opts,
        json!({"op":"add","group":"now","title":"<script>alert('x')</script> & \" 😀","note":"HIDDEN_SECRET"}),
    )?;
    edit(
        &opts,
        json!({"op":"add","group":"ideas","title":"Excluded"}),
    )?;
    let result = call(
        &opts,
        json!({"op":"snapshot","input":"list.json","output":"page.html","groups":["now","done"]}),
    )?;
    let html = fs::read_to_string(dir.path().join("page.html"))?;
    for forbidden in [
        "HIDDEN_SECRET",
        "Excluded",
        "<script",
        "onclick",
        "http:",
        "https:",
        "revision",
        "t1",
        "l1",
        " id=",
        "style=",
    ] {
        assert!(!html.contains(forbidden), "{forbidden}");
    }
    assert!(html.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; &quot; 😀"));
    assert!(html.contains("now <span"));
    assert!(html.contains("done <span"));
    assert_eq!(html.matches("<style>").count(), 1);
    let style = html
        .split_once("<style>")
        .ok_or("style")?
        .1
        .split_once("</style>")
        .ok_or("end style")?
        .0;
    let hash = Sha256::digest(style.as_bytes());
    // Decode independently through the system's base64 utility.
    let mut child = Command::new("base64")
        .arg("-w0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child.stdin.take().ok_or("stdin")?.write_all(&hash)?;
    let output = child.wait_with_output()?;
    assert!(output.status.success());
    assert_eq!(
        result["data"]["style_sha256"],
        String::from_utf8(output.stdout)?
    );
    let source = fs::read(dir.path().join("list.json"))?;
    let bad = call(
        &opts,
        json!({"op":"snapshot","input":"list.json","output":"list.json"}),
    );
    assert!(bad.is_err());
    assert_eq!(fs::read(dir.path().join("list.json"))?, source);
    Ok(())
}

#[test]
fn every_edit_has_its_own_summary() -> TestResult {
    let dir = tempfile::tempdir()?;
    let opts = options(dir.path())?;
    let created = call(
        &opts,
        json!({"op":"create","output":"list.json","title":" Launch "}),
    )?;
    assert_eq!(created["summary"], "Created the list \"Launch\".");
    let added = edit(
        &opts,
        json!({"op":"add","group":"now","title":"Send invoice"}),
    )?;
    assert_eq!(added["summary_code"], "tasks.added");
    assert_eq!(added["summary"], "Added \"Send invoice\".");
    let id = added["data"]["groups"][0]["tasks"][0]["id"].clone();
    let moved = edit(&opts, json!({"op":"move","id":id,"group":"done","index":0}))?;
    assert_eq!(moved["summary"], "Moved the task to done.");
    let renamed = edit(&opts, json!({"op":"rename","title":"Q4 launch"}))?;
    assert_eq!(renamed["summary"], "Renamed the list to \"Q4 launch\".");
    let read = call(&opts, json!({"op":"read","input":"list.json"}))?;
    assert_eq!(read["summary"], "Tasks in the list: 1.");
    Ok(())
}
