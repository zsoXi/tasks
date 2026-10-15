use serde_json::{Value, json};
use tasks_core::{Document, Edit, Timestamp, apply, new_list, parse, to_json, view};
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[derive(Clone)]
struct Step {
    id: String,
    title: String,
    done: bool,
}
#[derive(Clone)]
struct Task {
    id: String,
    title: String,
    note: Option<String>,
    steps: Vec<Step>,
}
#[derive(Clone)]
struct Reference {
    groups: [Vec<Task>; 6],
    revision: u64,
    next: u64,
    title: String,
}
impl Reference {
    fn new() -> Self {
        Self {
            groups: std::array::from_fn(|_| vec![]),
            revision: 0,
            next: 1,
            title: "List".into(),
        }
    }
    fn locate(&self, id: &str) -> Option<(usize, usize, Option<usize>)> {
        for (g, group) in self.groups.iter().enumerate() {
            for (t, task) in group.iter().enumerate() {
                if task.id == id {
                    return Some((g, t, None));
                }
                if let Some(s) = task.steps.iter().position(|s| s.id == id) {
                    return Some((g, t, Some(s)));
                }
            }
        }
        None
    }
    fn ids(&self) -> Vec<String> {
        self.groups
            .iter()
            .flatten()
            .flat_map(|t| std::iter::once(t.id.clone()).chain(t.steps.iter().map(|s| s.id.clone())))
            .collect()
    }
    fn counts(&self) -> (usize, usize) {
        (
            self.groups.iter().map(Vec::len).sum(),
            self.groups.iter().flatten().map(|t| t.steps.len()).sum(),
        )
    }
    fn shape(&self) -> Value {
        json!({"list":{"id":"l1","title":self.title,"revision":self.revision},"groups":self.groups.iter().enumerate().map(|(g,tasks)|json!({"key":(["now","later","ongoing","ideas","done","rejected"][g]),"tasks":tasks.iter().map(|t| {
            let mut v=json!({"id":t.id,"title":t.title,"subtasks":t.steps.iter().map(|s|json!({"id":s.id,"title":s.title,"done":s.done})).collect::<Vec<_>>()});
            if let Some(note)=&t.note {v["note"]=json!(note);}
            if !t.steps.is_empty(){v["progress"]=json!({"done":t.steps.iter().filter(|s|s.done).count(),"total":t.steps.len()});}v
        }).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
    fn perform(&mut self, v: &Value) -> Result<(), &'static str> {
        if v["expected_revision"] != self.revision {
            return Err("tasks.conflict");
        }
        let op = v["op"].as_str().ok_or("op")?;
        let id = v["id"].as_str().unwrap_or("");
        let target = || self.locate(id).ok_or("tasks.not_found");
        let clean_title = |s: &str| -> Result<String, &'static str> {
            let s = s.trim();
            if s.is_empty() {
                return Err("tasks.title_empty");
            }
            if s.chars().count() > 200 {
                return Err("tasks.title_too_long");
            }
            if s.chars().any(char::is_control) {
                return Err("tasks.control_character");
            }
            Ok(s.into())
        };
        match op {
            "add" => {
                let name = clean_title(v["title"].as_str().ok_or("title")?)?;
                let parent = v["parent"].as_str();
                let index = v["index"].as_u64().map(|n| n as usize);
                let note = v["note"].as_str().map(str::to_owned);
                let g = ["now", "later", "ongoing", "ideas", "done", "rejected"]
                    .iter()
                    .position(|g| v["group"] == *g)
                    .unwrap_or(0);
                if let Some(parent) = parent {
                    let (g, t, s) = self.locate(parent).ok_or("tasks.not_found")?;
                    if s.is_some() {
                        return Err("tasks.not_subtask");
                    }
                    if note.is_some() {
                        return Err("tasks.subtask_note");
                    }
                    if self.groups[g][t].steps.len() >= 50 {
                        return Err("tasks.too_many_subtasks");
                    }
                    let i = index.unwrap_or(self.groups[g][t].steps.len());
                    if i > self.groups[g][t].steps.len() {
                        return Err("tasks.index_out_of_range");
                    }
                    self.groups[g][t].steps.insert(
                        i,
                        Step {
                            id: format!("t{}", self.next),
                            title: name,
                            done: false,
                        },
                    );
                } else {
                    if self.counts().0 >= 500 {
                        return Err("tasks.too_many_tasks");
                    }
                    let i = index.unwrap_or(self.groups[g].len());
                    if i > self.groups[g].len() {
                        return Err("tasks.index_out_of_range");
                    }
                    self.groups[g].insert(
                        i,
                        Task {
                            id: format!("t{}", self.next),
                            title: name,
                            note,
                            steps: vec![],
                        },
                    );
                }
                self.next += 1;
            }
            "rename" => self.title = clean_title(v["title"].as_str().ok_or("title")?)?,
            "edit" => {
                let (g, t, s) = target()?;
                let title = v["title"].as_str().map(clean_title).transpose()?;
                if let Some(s) = s {
                    if v.get("note").is_some() {
                        return Err("tasks.subtask_note");
                    }
                    if let Some(title) = title {
                        self.groups[g][t].steps[s].title = title;
                    }
                } else {
                    if let Some(title) = title {
                        self.groups[g][t].title = title;
                    }
                    if let Some(n) = v.get("note") {
                        self.groups[g][t].note = n.as_str().map(str::to_owned);
                    }
                }
            }
            "check" => {
                let (g, t, s) = target()?;
                let s = s.ok_or("tasks.not_subtask")?;
                self.groups[g][t].steps[s].done = v["done"].as_bool().ok_or("done")?;
            }
            "delete" => {
                let (g, t, s) = target()?;
                if let Some(s) = s {
                    self.groups[g][t].steps.remove(s);
                } else {
                    self.groups[g].remove(t);
                }
            }
            "move" => {
                let (g, t, s) = target()?;
                if s.is_some() {
                    return Err("tasks.not_subtask");
                }
                let to = ["now", "later", "ongoing", "ideas", "done", "rejected"]
                    .iter()
                    .position(|g| v["group"] == *g)
                    .ok_or("group")?;
                let i = v["index"].as_u64().ok_or("index")? as usize;
                if i > self.groups[to].len() - usize::from(g == to) {
                    return Err("tasks.index_out_of_range");
                }
                let task = self.groups[g].remove(t);
                self.groups[to].insert(i, task);
            }
            "indent" => {
                let (g, t, s) = target()?;
                if s.is_some() {
                    return Err("tasks.not_subtask");
                }
                if t == 0 {
                    return Err("tasks.no_task_above");
                }
                if !self.groups[g][t].steps.is_empty() {
                    return Err("tasks.has_subtasks");
                }
                if self.groups[g][t]
                    .note
                    .as_ref()
                    .is_some_and(|s| !s.is_empty())
                {
                    return Err("tasks.has_note");
                }
                if self.groups[g][t - 1].steps.len() >= 50 {
                    return Err("tasks.too_many_subtasks");
                }
                let task = self.groups[g].remove(t);
                self.groups[g][t - 1].steps.push(Step {
                    id: task.id,
                    title: task.title,
                    done: false,
                });
            }
            "outdent" => {
                let (g, t, s) = target()?;
                let s = s.ok_or("tasks.not_subtask")?;
                if self.counts().0 >= 500 {
                    return Err("tasks.too_many_tasks");
                }
                let step = self.groups[g][t].steps.remove(s);
                self.groups[g].insert(
                    t + 1,
                    Task {
                        id: step.id,
                        title: step.title,
                        note: None,
                        steps: vec![],
                    },
                );
            }
            _ => return Err("op"),
        }
        self.revision += 1;
        Ok(())
    }
}
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick(&mut self, n: usize) -> usize {
        self.next() as usize % n
    }
}
fn execute_sequence(seed: u64) -> TestResult {
    let mut rng = Random(seed);
    let now = Timestamp::new("2026-10-15T18:00:00Z")?;
    let mut doc = new_list("l1", "List", now.clone())?;
    let mut reference = Reference::new();
    for step in 0..64 {
        let ids = reference.ids();
        let id = if ids.is_empty() || rng.pick(9) == 0 {
            "missing".into()
        } else {
            ids[rng.pick(ids.len())].clone()
        };
        let group = ["now", "later", "ongoing", "ideas", "done", "rejected"][rng.pick(6)];
        let name = [
            "A",
            "<script>&\"'",
            "😀🦀",
            "\u{202e}bidi",
            "zero\u{200b}width",
            "  trimmed  ",
            "",
            "bad\u{0001}title",
        ][rng.pick(8)];
        let op = [
            "add", "add", "add", "edit", "check", "move", "indent", "outdent", "delete", "rename",
        ][rng.pick(10)];
        let mut v = json!({"op":op,"expected_revision":reference.revision});
        if rng.pick(23) == 0 {
            v["expected_revision"] = json!(reference.revision + 1);
        }
        match op {
            "add" => {
                v["title"] = json!(name);
                v["group"] = json!(group);
                if rng.pick(3) == 0 {
                    v["parent"] = json!(id);
                }
                if rng.pick(4) == 0 {
                    v["index"] = json!(rng.pick(4));
                }
                if rng.pick(5) == 0 {
                    v["note"] = json!("Note");
                }
            }
            "edit" => {
                v["id"] = json!(id);
                if rng.pick(2) == 0 {
                    v["title"] = json!(name);
                }
                if rng.pick(3) == 0 {
                    v["note"] = if rng.pick(2) == 0 {
                        Value::Null
                    } else {
                        json!("Note")
                    };
                }
            }
            "rename" => v["title"] = json!(name),
            _ => {
                v["id"] = json!(id);
                if op == "check" {
                    v["done"] = json!(rng.pick(2) == 0);
                }
                if op == "move" {
                    v["group"] = json!(group);
                    v["index"] = json!(rng.pick(5));
                }
            }
        }
        let edit: Edit = serde_json::from_value(v.clone())?;
        let bytes = to_json(&doc);
        let revision = doc.revision();
        let mut candidate = reference.clone();
        let expected = candidate.perform(&v);
        let actual = apply(&doc, &edit, now.clone());
        assert_eq!(
            actual.is_ok(),
            expected.is_ok(),
            "seed={seed} step={step} edit={v} actual={actual:?} expected={expected:?}"
        );
        match actual {
            Ok(updated) => {
                assert_eq!(updated.revision(), revision + 1, "seed={seed}");
                doc = updated;
                reference = candidate;
            }
            Err(error) => {
                assert_eq!(
                    Some(error.code.code()),
                    expected.err(),
                    "seed={seed} edit={v}"
                );
                assert_eq!(to_json(&doc), bytes, "seed={seed}");
                assert_eq!(doc.revision(), revision);
            }
        }
        assert_eq!(view(&doc), reference.shape(), "seed={seed} step={step}");
        let shape = view(&doc);
        let tasks = shape["groups"]
            .as_array()
            .ok_or("groups")?
            .iter()
            .flat_map(|g| g["tasks"].as_array().into_iter().flatten())
            .collect::<Vec<_>>();
        assert_eq!(tasks.len(), reference.counts().0, "seed={seed}");
        assert_eq!(
            tasks
                .iter()
                .map(|t| t["subtasks"].as_array().map_or(0, Vec::len))
                .sum::<usize>(),
            reference.counts().1,
            "seed={seed}"
        );
        assert_eq!(
            to_json(&parse(&to_json(&doc))?),
            to_json(&doc),
            "seed={seed}"
        );
        let snapshot = tasks_core::snapshot_html(&doc, &Default::default());
        assert!(!snapshot.html.contains("<script>"), "seed={seed}");
    }
    Ok(())
}
#[test]
fn i1_i3_two_thousand_seeded_sequences() -> TestResult {
    for seed in 1..=2000 {
        execute_sequence(seed)?;
    }
    Ok(())
}

fn full_document(tasks: usize, subtasks: usize) -> Result<Document, Box<dyn std::error::Error>> {
    let now = Timestamp::new("2026-10-15T18:00:00Z")?;
    let mut v: Value = serde_json::from_str(&to_json(&new_list("l1", "Full", now)?))?;
    let mut next = 1;
    let mut all = vec![];
    for _ in 0..tasks {
        let id = format!("t{next}");
        next += 1;
        let mut subs = vec![];
        for _ in 0..subtasks {
            subs.push(json!({"id":format!("t{next}"),"title":"Step","done":false}));
            next += 1;
        }
        all.push(json!({"id":id,"title":"Task","created_at":"2026-10-15T18:00:00Z","updated_at":"2026-10-15T18:00:00Z","subtasks":subs}));
    }
    v["groups"][0]["tasks"] = json!(all);
    v["next_id"] = json!(next);
    Ok(parse(&v.to_string())?)
}
#[test]
fn hostile_documents_limits_and_deserialization() -> TestResult {
    let now = Timestamp::new("2026-10-15T18:00:00Z")?;
    let mut base: Value = serde_json::from_str(&to_json(&full_document(2, 1)?))?;
    base["groups"][0]["tasks"][0]["note"] = json!("Existing note");
    for (pointer, value, code) in [
        ("/format", json!("other"), "tasks.format"),
        ("/title", json!(" "), "tasks.title_empty"),
        ("/title", json!("a".repeat(201)), "tasks.title_too_long"),
        (
            "/groups/0/tasks/0/title",
            json!("bad\u{0}title"),
            "tasks.control_character",
        ),
        (
            "/groups/0/tasks/0/note",
            json!("a".repeat(2001)),
            "tasks.note_too_long",
        ),
        (
            "/groups/0/tasks/0/subtasks/0/title",
            json!("a".repeat(201)),
            "tasks.title_too_long",
        ),
        ("/groups/0/tasks/1/id", json!("t1"), "tasks.invalid_ids"),
        ("/groups/0/tasks/0/id", json!("t01"), "tasks.invalid_ids"),
        (
            "/created_at",
            json!("2026-02-29T00:00:00Z"),
            "tasks.validation",
        ),
    ] {
        let mut v = base.clone();
        *v.pointer_mut(pointer).ok_or("pointer")? = value;
        let result = parse(&v.to_string());
        assert!(result.is_err(), "{pointer}");
        if let Err(e) = result {
            assert_eq!(e.code.code(), code, "{pointer}");
        }
        assert!(
            serde_json::from_value::<Document>(v).is_err(),
            "deserialize {pointer}"
        );
    }
    let mut missing = base.clone();
    missing["groups"].as_array_mut().ok_or("groups")?.pop();
    assert_eq!(
        parse(&missing.to_string()).err().map(|e| e.code.code()),
        Some("tasks.groups")
    );
    for (tasks, subs, code) in [
        (501, 0, "tasks.too_many_tasks"),
        (1, 51, "tasks.too_many_subtasks"),
    ] {
        let result = full_document(tasks, subs);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains(if code == "tasks.too_many_tasks" {
                "500"
            } else {
                "50"
            }));
        }
    }
    let full = full_document(500, 1)?;
    for v in [
        json!({"op":"add","expected_revision":0,"group":"now","title":"Extra"}),
        json!({"op":"outdent","expected_revision":0,"id":"t2"}),
    ] {
        let result = apply(&full, &serde_json::from_value(v)?, now.clone());
        assert_eq!(
            result.err().map(|e| e.code.code()),
            Some("tasks.too_many_tasks")
        );
    }
    let doc = new_list("l1", &"😀".repeat(200), now.clone())?;
    assert_eq!(parse(&to_json(&doc))?.revision(), 0);
    let v: Value = serde_json::from_str(&to_json(&doc))?;
    let mut max = v.clone();
    max["revision"] = json!(u64::MAX);
    let d = parse(&max.to_string())?;
    assert!(
        apply(
            &d,
            &Edit::Rename {
                expected_revision: u64::MAX,
                title: "Next".into()
            },
            now.clone()
        )
        .is_err()
    );
    let mut exhausted = v;
    exhausted["next_id"] = json!(u64::MAX);
    let d = parse(&exhausted.to_string())?;
    assert!(
        apply(
            &d,
            &Edit::Add {
                expected_revision: 0,
                group: Some(tasks_core::GroupKey::Now),
                title: "Next".into(),
                note: None,
                index: None,
                parent: None
            },
            now
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn release_full_list_performance() -> TestResult {
    let doc = full_document(500, 10)?;
    let edit = Edit::Rename {
        expected_revision: 0,
        title: "Renamed".into(),
    };
    let now = Timestamp::new("2026-10-15T18:00:00Z")?;
    let mut measurements = vec![];
    let mut bytes = 0;
    for _ in 0..31 {
        let start = std::time::Instant::now();
        let updated = apply(std::hint::black_box(&doc), &edit, now.clone())?;
        let encoded = to_json(&updated);
        bytes = encoded.len();
        std::hint::black_box(encoded);
        measurements.push(start.elapsed());
    }
    measurements.sort();
    eprintln!(
        "PERFORMANCE: 500 tasks x 10 subtasks, bytes={bytes}, median={:?}, max={:?}",
        measurements[15], measurements[30]
    );
    if !cfg!(debug_assertions) {
        assert!(measurements[15] < std::time::Duration::from_millis(5));
    }
    Ok(())
}
