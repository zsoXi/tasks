//! Form-friendly request descriptions shared by the CLI and library.
use crate::{Error, GroupKey, Timestamp, messages::MessageCode as C};
use serde_json::{Value, json};

pub fn invalid(field: &str) -> Error {
    Error::new(C::Validation, json!({"field":field}))
}
fn text(title: &str, description: &str) -> Value {
    json!({"type":"string","title":title,"description":description})
}
fn integer(title: &str, description: &str) -> Value {
    json!({"type":"integer","minimum":0,"title":title,"description":description})
}
fn boolean(title: &str, description: &str, default: bool) -> Value {
    json!({"type":"boolean","title":title,"description":description,"default":default})
}
fn object(title: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","title":title,"description":description,"properties":properties,"required":required,"additionalProperties":false})
}
fn array(title: &str, description: &str, items: Value) -> Value {
    json!({"type":"array","title":title,"description":description,"items":items})
}
fn group() -> Value {
    let mut v = text("Group", "Choose where this task belongs.");
    v["enum"] = json!(GroupKey::ALL.map(GroupKey::as_str));
    v
}
fn file(output: bool, html: bool) -> Value {
    let mut v = text(
        if output { "Save as" } else { "List file" },
        if output {
            "Choose a file name in your working folder."
        } else {
            "Choose a saved list in your working folder."
        },
    );
    v["x-file"] = json!(if output { "output" } else { "input" });
    if output {
        v["x-formats"] = json!([if html { "html" } else { "json" }]);
    } else {
        v["x-accept"] = json!([".json"]);
    }
    v
}
pub fn schema() -> Value {
    let mut ops = Vec::new();
    for (op, description) in [
        ("create", "Start a new task list."),
        ("read", "Open a saved task list."),
        ("add", "Add a task or a subtask."),
        ("edit", "Change a title or note."),
        ("check", "Mark a subtask complete or incomplete."),
        ("move", "Move or reorder a task."),
        ("indent", "Make a task a subtask of the one above."),
        ("outdent", "Promote a subtask to a task."),
        ("delete", "Remove a task or subtask."),
        ("rename", "Change the list title."),
        ("snapshot", "Save a read-only preview page."),
    ] {
        let mut p =
            json!({"op":{"type":"string","const":op,"title":"Action","description":description}});
        let mut required = vec!["op"];
        if op == "create" {
            p["output"] = file(true, false);
            p["title"] = text("List title", "Give your list a short name.");
            required.extend(["output", "title"]);
        } else {
            p["input"] = file(false, false);
            required.push("input");
            if !matches!(op, "read" | "snapshot") {
                p["expected_revision"] = integer(
                    "Saved version",
                    "Use the version shown when you last opened the list.",
                );
                required.push("expected_revision");
            }
        }
        if matches!(
            op,
            "edit" | "check" | "move" | "indent" | "outdent" | "delete"
        ) {
            p["id"] = text("Task", "Choose the task or subtask to change.");
            required.push("id");
        }
        if matches!(op, "add" | "edit" | "rename") {
            p["title"] = text("Title", "Enter a short title, up to 200 characters.");
            if op != "edit" {
                required.push("title");
            }
        }
        if matches!(op, "add" | "edit") {
            p["note"] = text(
                "Note",
                "Add details, up to 2,000 characters. Leave empty to clear them.",
            );
            p["note"]["type"] = json!(["string", "null"]);
        }
        if matches!(op, "add" | "move") {
            p["group"] = group();
            p["index"] = integer(
                "Position",
                "Choose a position, starting at zero. For a new task, leave blank to place it last.",
            );
            if op == "move" {
                required.extend(["group", "index"]);
            }
        }
        if op == "add" {
            p["parent"] = text(
                "Parent task",
                "Choose a task to add a subtask instead. The group choice is then ignored.",
            );
        }
        if op == "check" {
            p["done"] = boolean("Complete", "Choose whether the subtask is finished.", false);
            required.push("done");
        }
        if op == "snapshot" {
            p["output"] = file(true, true);
            required.push("output");
            p["groups"] = array(
                "Included groups",
                "Choose groups to include, or leave blank for all six.",
                group(),
            );
            p["show_notes"] = boolean("Show notes", "Include task notes in the preview.", false);
            p["show_subtasks"] = boolean(
                "Show subtasks",
                "Include the subtasks below each task.",
                true,
            );
        }
        ops.push(object(op, description, p, &required));
    }
    json!({"title":"Tasks","description":"Keep a small task list in six groups.","oneOf":ops})
}

pub fn validate_request(v: &Value) -> Result<(), Error> {
    let s = schema();
    let op = v
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("/op"))?;
    let forms = s["oneOf"].as_array().ok_or_else(|| invalid(""))?;
    let form = forms.iter().find(|f| f["properties"]["op"]["const"] == op)
        .ok_or_else(|| Error::new(C::Enum, json!({"field":"/op","allowed":forms.iter().map(|f| f["properties"]["op"]["const"].clone()).collect::<Vec<_>>()})))?;
    validate(v, form, "")
}
fn escape_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}
pub fn validate(v: &Value, s: &Value, path: &str) -> Result<(), Error> {
    let matches_type = |t: &str| match t {
        "string" => v.is_string(),
        "integer" => v.as_u64().is_some(),
        "boolean" => v.is_boolean(),
        "object" => v.is_object(),
        "array" => v.is_array(),
        "null" => v.is_null(),
        _ => false,
    };
    let valid = match &s["type"] {
        Value::String(t) => matches_type(t),
        Value::Array(types) => types.iter().filter_map(Value::as_str).any(matches_type),
        _ => true,
    };
    if !valid {
        return Err(invalid(path));
    }
    if let Some(allowed) = s.get("enum")
        && !allowed.as_array().is_some_and(|a| a.contains(v))
    {
        return Err(Error::new(C::Enum, json!({"field":path,"allowed":allowed})));
    }
    if let Some(c) = s.get("const")
        && v != c
    {
        return Err(invalid(path));
    }
    if s["format"] == "date-time"
        && let Some(value) = v.as_str()
    {
        Timestamp::new(value).map_err(|_| invalid(path))?;
    }
    if let Some(values) = v.as_object() {
        if let Some(required) = s["required"].as_array() {
            for name in required.iter().filter_map(Value::as_str) {
                if !values.contains_key(name) {
                    return Err(invalid(&format!("{path}/{}", escape_pointer(name))));
                }
            }
        }
        for (name, value) in values {
            let p = format!("{path}/{}", escape_pointer(name));
            if let Some(field) = s["properties"].get(name) {
                validate(value, field, &p)?;
            } else if s["additionalProperties"] == false {
                return Err(invalid(&p));
            }
        }
    }
    if let Some(values) = v.as_array() {
        for (i, value) in values.iter().enumerate() {
            validate(value, &s["items"], &format!("{path}/{i}"))?;
        }
    }
    Ok(())
}
pub(crate) fn document_schema() -> Value {
    let mut time = text("Time", "When the list was changed.");
    time["format"] = json!("date-time");
    let sub = object(
        "Subtask",
        "One step in a task.",
        json!({"id":text("Identifier","A saved task reference."),"title":text("Title","A short task name."),"done":boolean("Complete","Whether the step is finished.",false)}),
        &["id", "title", "done"],
    );
    let mut n = text("Note", "Task details.");
    n["type"] = json!(["string", "null"]);
    let task = object(
        "Task",
        "A task and its steps.",
        json!({"id":text("Identifier","A saved task reference."),"title":text("Title","A short task name."),"note":n,"created_at":time,"updated_at":time,"subtasks":array("Subtasks","Steps in this task.",sub)}),
        &["id", "title", "created_at", "updated_at", "subtasks"],
    );
    let groups = array(
        "Groups",
        "The six task groups.",
        object(
            "Group",
            "Tasks in one group.",
            json!({"key":group(),"tasks":array("Tasks","Tasks in their saved order.",task)}),
            &["key", "tasks"],
        ),
    );
    object(
        "List",
        "A saved task list.",
        json!({"format":text("Format","The list format."),"id":text("Identifier","A saved list reference."),"title":text("Title","The list name."),"revision":integer("Version","The saved version."),"next_id":integer("Next identifier","The next available task number."),"created_at":time,"updated_at":time,"groups":groups}),
        &[
            "format",
            "id",
            "title",
            "revision",
            "next_id",
            "created_at",
            "updated_at",
            "groups",
        ],
    )
}
