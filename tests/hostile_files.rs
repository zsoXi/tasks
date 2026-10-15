use serde_json::{Value, json};
use tasks::{Options, parse_input, run};
use tasks_core::{Timestamp, new_list, to_json};
#[test]
fn hand_edited_documents_are_rejected_without_changing_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let time = Timestamp::new("2026-10-15T18:00:00Z")?;
    let options = Options {
        base_dir: folder.path().into(),
        now: Some(time.clone()),
        ..Options::default()
    };
    let base: Value = serde_json::from_str(&to_json(&new_list("l1", "List", time)?))?;
    let task = json!({"id":"t1","title":"Task","created_at":"2026-10-15T18:00:00Z","updated_at":"2026-10-15T18:00:00Z","subtasks":[]});
    for (kind, code) in [
        ("format", "tasks.format"),
        ("groups", "tasks.groups"),
        ("duplicate", "tasks.invalid_ids"),
        ("tasks", "tasks.too_many_tasks"),
        ("subtasks", "tasks.too_many_subtasks"),
        ("title", "tasks.title_too_long"),
        ("note", "tasks.note_too_long"),
    ] {
        let mut v = base.clone();
        v["next_id"] = json!(1000);
        v["groups"][0]["tasks"] = json!([task.clone()]);
        match kind {
            "format" => v["format"] = json!("tasks/2"),
            "groups" => {
                v["groups"].as_array_mut().ok_or("groups")?.pop();
            }
            "duplicate" => v["groups"][0]["tasks"] = json!([task.clone(), task.clone()]),
            "tasks" => {
                v["groups"][0]["tasks"] = json!(
                    (1..=501)
                        .map(|i| {
                            let mut t = task.clone();
                            t["id"] = json!(format!("t{i}"));
                            t
                        })
                        .collect::<Vec<_>>()
                )
            }
            "subtasks" => {
                v["groups"][0]["tasks"][0]["subtasks"] = json!(
                    (2..=52)
                        .map(|i| json!({"id":format!("t{i}"),"title":"Step","done":false}))
                        .collect::<Vec<_>>()
                )
            }
            "title" => v["groups"][0]["tasks"][0]["title"] = json!("😀".repeat(201)),
            "note" => v["groups"][0]["tasks"][0]["note"] = json!("x".repeat(2001)),
            _ => return Err("unknown test fixture".into()),
        }
        let bytes = v.to_string();
        std::fs::write(folder.path().join("list.json"), &bytes)?;
        let result = run(
            &parse_input(
                r#"{"op":"rename","input":"list.json","expected_revision":0,"title":"Cannot apply"}"#,
            )?,
            &options,
        );
        assert_eq!(result.err().map(|e| e.0.code.code()), Some(code), "{kind}");
        assert_eq!(
            std::fs::read_to_string(folder.path().join("list.json"))?,
            bytes
        );
    }
    Ok(())
}
