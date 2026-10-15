//! File-backed task lists with optimistic revisions and descriptor-relative atomic writes.
mod files;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
pub use tasks_core;
use tasks_core::{
    Document, Edit, GroupKey, NotePatch, SnapshotOptions, Timestamp,
    messages::{self, MessageCode as C, Warning},
};
pub const NAME: &str = "tasks";
pub const DESCRIPTION: &str = "Keep a small task list with subtasks in six fixed groups.";
pub fn schema() -> Value {
    tasks_core::schema()
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(#[from] pub tasks_core::Error);
impl Error {
    pub fn response(&self, locale: &str) -> Output {
        Output {
            ok: false,
            summary: messages::render(self.0.code, &self.0.params, locale),
            summary_code: self.0.code.code(),
            summary_params: self.0.params.clone(),
            warnings: vec![],
            data: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Options {
    pub base_dir: PathBuf,
    pub max_input_bytes: u64,
    pub locale: String,
    pub now: Option<Timestamp>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from("."),
            max_input_bytes: 1_048_576,
            locale: "en".into(),
            now: None,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Output {
    pub ok: bool,
    pub summary: String,
    pub summary_code: &'static str,
    pub summary_params: Value,
    pub warnings: Vec<Warning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}
impl Output {
    fn success(
        data: Value,
        warnings: Vec<Warning>,
        (code, params): (C, Value),
        locale: &str,
    ) -> Self {
        Self {
            ok: true,
            summary: messages::render(code, &params, locale),
            summary_code: code.code(),
            summary_params: params,
            warnings,
            data: Some(data),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum Input {
    Create {
        output: PathBuf,
        title: String,
    },
    Read {
        input: PathBuf,
    },
    Add {
        input: PathBuf,
        expected_revision: u64,
        #[serde(default)]
        group: Option<GroupKey>,
        title: String,
        #[serde(default)]
        note: Option<String>,
        #[serde(default)]
        index: Option<usize>,
        #[serde(default)]
        parent: Option<String>,
    },
    Edit {
        input: PathBuf,
        expected_revision: u64,
        id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        note: NotePatch,
    },
    Check {
        input: PathBuf,
        expected_revision: u64,
        id: String,
        done: bool,
    },
    Move {
        input: PathBuf,
        expected_revision: u64,
        id: String,
        group: GroupKey,
        index: usize,
    },
    Indent {
        input: PathBuf,
        expected_revision: u64,
        id: String,
    },
    Outdent {
        input: PathBuf,
        expected_revision: u64,
        id: String,
    },
    Delete {
        input: PathBuf,
        expected_revision: u64,
        id: String,
    },
    Rename {
        input: PathBuf,
        expected_revision: u64,
        title: String,
    },
    Snapshot {
        input: PathBuf,
        output: PathBuf,
        #[serde(default)]
        groups: Option<Vec<GroupKey>>,
        #[serde(default)]
        show_notes: bool,
        #[serde(default = "yes")]
        show_subtasks: bool,
    },
}
fn yes() -> bool {
    true
}
pub fn parse_input(text: &str) -> Result<Input, Error> {
    let v: Value = serde_json::from_str(text).map_err(|e| tasks_core::Error::caused(C::Json, e))?;
    tasks_core::forms::validate_request(&v)?;
    serde_json::from_value(v).map_err(|e| tasks_core::Error::caused(C::Validation, e).into())
}
impl Input {
    fn file(&self) -> &std::path::Path {
        match self {
            Self::Create { output, .. } => output,
            Self::Read { input }
            | Self::Add { input, .. }
            | Self::Edit { input, .. }
            | Self::Check { input, .. }
            | Self::Move { input, .. }
            | Self::Indent { input, .. }
            | Self::Outdent { input, .. }
            | Self::Delete { input, .. }
            | Self::Rename { input, .. }
            | Self::Snapshot { input, .. } => input,
        }
    }
    fn edit(&self) -> Option<Edit> {
        Some(match self {
            Self::Add {
                expected_revision,
                group,
                title,
                note,
                index,
                parent,
                ..
            } => Edit::Add {
                expected_revision: *expected_revision,
                group: *group,
                title: title.clone(),
                note: note.clone(),
                index: *index,
                parent: parent.clone(),
            },
            Self::Edit {
                expected_revision,
                id,
                title,
                note,
                ..
            } => Edit::Edit {
                expected_revision: *expected_revision,
                id: id.clone(),
                title: title.clone(),
                note: note.clone(),
            },
            Self::Check {
                expected_revision,
                id,
                done,
                ..
            } => Edit::Check {
                expected_revision: *expected_revision,
                id: id.clone(),
                done: *done,
            },
            Self::Move {
                expected_revision,
                id,
                group,
                index,
                ..
            } => Edit::Move {
                expected_revision: *expected_revision,
                id: id.clone(),
                group: *group,
                index: *index,
            },
            Self::Indent {
                expected_revision,
                id,
                ..
            } => Edit::Indent {
                expected_revision: *expected_revision,
                id: id.clone(),
            },
            Self::Outdent {
                expected_revision,
                id,
                ..
            } => Edit::Outdent {
                expected_revision: *expected_revision,
                id: id.clone(),
            },
            Self::Delete {
                expected_revision,
                id,
                ..
            } => Edit::Delete {
                expected_revision: *expected_revision,
                id: id.clone(),
            },
            Self::Rename {
                expected_revision,
                title,
                ..
            } => Edit::Rename {
                expected_revision: *expected_revision,
                title: title.clone(),
            },
            _ => return None,
        })
    }
}
fn now(opts: &Options) -> Result<Timestamp, Error> {
    if let Some(now) = &opts.now {
        return Ok(now.clone());
    }
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| tasks_core::Error::caused(C::Clock, e))?
        .as_secs();
    if seconds > 253_402_300_799 {
        return Err(tasks_core::Error::new(C::Clock, json!({})).into());
    }
    // Gregorian civil date from days since the Unix epoch.
    let days = seconds / 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    year += u64::from(month <= 2);
    Timestamp::new(&format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    ))
    .map_err(Into::into)
}
fn save(target: &files::Target, doc: &Document, opts: &Options) -> Result<(), Error> {
    let json = tasks_core::to_json(doc);
    if json.len() as u64 > opts.max_input_bytes {
        return Err(tasks_core::Error::new(C::Size, json!({"limit":opts.max_input_bytes})).into());
    }
    target.write(json.as_bytes())
}
pub fn run(input: &Input, opts: &Options) -> Result<Output, Error> {
    let target = files::Target::open(&opts.base_dir, input.file())?;
    // Readers need no lock: rename exposes either complete version. All writers share this directory lock.
    if !matches!(input, Input::Read { .. } | Input::Snapshot { .. }) {
        target.lock()?;
    }
    if let Input::Create { title, .. } = input {
        let doc = tasks_core::new_list("l1", title, now(opts)?)?;
        save(&target, &doc, opts)?;
        return Ok(Output::success(
            tasks_core::view(&doc),
            vec![],
            (C::Created, json!({"title": title.trim()})),
            &opts.locale,
        ));
    }
    let doc = tasks_core::parse(&target.read(opts.max_input_bytes)?)?;
    if let Input::Snapshot {
        output,
        groups,
        show_notes,
        show_subtasks,
        ..
    } = input
    {
        let snapshot = tasks_core::snapshot_html(
            &doc,
            &SnapshotOptions {
                groups: groups.clone(),
                show_notes: *show_notes,
                show_subtasks: *show_subtasks,
            },
        );
        let out = files::Target::open(&opts.base_dir, output)?;
        // Do not replace the source JSON with HTML. Rename breaks hard-link aliases without changing the source inode.
        if output == input.file() {
            return Err(tasks_core::Error::new(C::Path, json!({})).into());
        }
        out.lock()?;
        out.write(snapshot.html.as_bytes())?;
        return Ok(Output::success(
            json!({"style_sha256":snapshot.style_sha256}),
            vec![],
            (C::Snapshot, json!({})),
            &opts.locale,
        ));
    }
    if let Some(edit) = input.edit() {
        let (doc, warnings) =
            tasks_core::apply_with_warnings(&doc, &edit, now(opts)?, &opts.locale)?;
        save(&target, &doc, opts)?;
        let data = tasks_core::view(&doc);
        let summary = edit_summary(input, &data);
        Ok(Output::success(data, warnings, summary, &opts.locale))
    } else {
        let data = tasks_core::view(&doc);
        let tasks: usize = data["groups"].as_array().map_or(0, |groups| {
            groups
                .iter()
                .filter_map(|g| g["tasks"].as_array())
                .map(Vec::len)
                .sum()
        });
        Ok(Output::success(
            data,
            vec![],
            (C::Read, json!({"tasks": tasks})),
            &opts.locale,
        ))
    }
}

/// One plain sentence per edit, so a chat assistant can say what changed.
fn edit_summary(input: &Input, data: &Value) -> (C, Value) {
    match input {
        Input::Add { title, .. } => (C::Added, json!({"title": title.trim()})),
        Input::Edit { .. } => (C::Edited, json!({})),
        Input::Check { .. } => (C::Checked, json!({})),
        Input::Move { group, .. } => (C::Moved, json!({"group": group})),
        Input::Indent { .. } => (C::Indented, json!({})),
        Input::Outdent { .. } => (C::Outdented, json!({})),
        Input::Delete { .. } => (C::Deleted, json!({})),
        Input::Rename { .. } => (C::Renamed, json!({"title": data["list"]["title"]})),
        Input::Create { .. } | Input::Read { .. } | Input::Snapshot { .. } => {
            (C::Edited, json!({}))
        }
    }
}
