use serde_json::{Value, json};
use tasks_core::{Edit, Timestamp, apply, new_list, parse, to_json};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn perform(
    doc: &tasks_core::Document,
    v: Value,
) -> Result<tasks_core::Document, Box<dyn std::error::Error>> {
    let edit: Edit = serde_json::from_value(v)?;
    Ok(apply(doc, &edit, Timestamp::new("2026-10-15T18:00:00Z")?)?)
}
#[test]
fn edit_limits_reject_without_changes_and_report_limits() -> TestResult {
    let now = Timestamp::new("2026-10-15T18:00:00Z")?;
    let mut doc = new_list("l1", "List", now)?;
    for v in [
        json!({"op":"add","group":"now","title":"A"}),
        json!({"op":"add","group":"now","title":"B"}),
        json!({"op":"add","parent":"t1","title":"Step"}),
    ] {
        let mut v = v;
        v["expected_revision"] = json!(doc.revision());
        doc = perform(&doc, v)?;
    }
    for (mut v, code, limit) in [
        (json!({"op":"rename","title":" "}), "tasks.title_empty", 200),
        (
            json!({"op":"edit","id":"t1","title":"a".repeat(201)}),
            "tasks.title_too_long",
            200,
        ),
        (
            json!({"op":"edit","id":"t3","title":"a".repeat(201)}),
            "tasks.title_too_long",
            200,
        ),
        (
            json!({"op":"add","group":"ideas","title":"a".repeat(201)}),
            "tasks.title_too_long",
            200,
        ),
        (
            json!({"op":"edit","id":"t1","note":"a".repeat(2001)}),
            "tasks.note_too_long",
            2000,
        ),
        (
            json!({"op":"add","group":"ideas","title":"X","note":"a".repeat(2001)}),
            "tasks.note_too_long",
            2000,
        ),
        (
            json!({"op":"edit","id":"t1","title":"a\nb"}),
            "tasks.control_character",
            200,
        ),
        (
            json!({"op":"edit","id":"t1","note":"bad\u{0001}note"}),
            "tasks.control_character",
            2000,
        ),
    ] {
        v["expected_revision"] = json!(doc.revision());
        let before = to_json(&doc);
        let edit: Edit = serde_json::from_value(v)?;
        let result = apply(&doc, &edit, Timestamp::new("2026-10-15T18:00:00Z")?);
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.code.code(), code);
            assert_eq!(e.params["limit"], limit);
            assert!(!e.to_string().contains('{'));
        }
        assert_eq!(to_json(&doc), before);
    }
    let mut v = json!({"op":"edit","id":"t1","title":"😀".repeat(200),"note":"n".repeat(2000)});
    v["expected_revision"] = json!(doc.revision());
    doc = perform(&doc, v)?;
    parse(&to_json(&doc))?;
    let mut v = json!({"op":"edit","id":"t2","note":""});
    v["expected_revision"] = json!(doc.revision());
    doc = perform(&doc, v)?;
    let edit: Edit = serde_json::from_value(
        json!({"op":"indent","id":"t2","expected_revision":doc.revision()}),
    )?;
    assert_eq!(
        apply(&doc, &edit, Timestamp::new("2026-10-15T18:00:00Z")?)
            .err()
            .map(|e| e.code.code()),
        Some("tasks.has_note")
    );
    Ok(())
}
#[test]
fn document_metadata_and_nested_shape_rejected() -> TestResult {
    let now = Timestamp::new("2026-10-15T18:00:00Z")?;
    let base: Value = serde_json::from_str(&to_json(&new_list("l1", "List", now)?))?;
    for field in ["people", "tenants", "visibility", "owner"] {
        let mut v = base.clone();
        v[field] = json!("private");
        let e = parse(&v.to_string()).err().ok_or("accepted metadata")?;
        assert_eq!(e.params["field"], format!("/{field}"));
    }
    for value in [
        "2026-01-01T24:00:00Z",
        "2026-00-01T00:00:00Z",
        "2026-01-01T00:00:00+00:00",
        "+026-01-01T00:00:00Z",
    ] {
        assert!(Timestamp::new(value).is_err());
    }
    Ok(())
}
