use serde::Serialize;
use serde_json::{Value, json};

macro_rules! catalogue {
    ($( $variant:ident => ($code:literal, $kind:literal, $text:literal, [$($param:literal),*]) ),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum MessageCode { $($variant),* }
        impl MessageCode {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];
            pub fn code(self) -> &'static str { match self { $(Self::$variant => $code),* } }
            pub fn template(self) -> &'static str { match self { $(Self::$variant => $text),* } }
        }
        pub fn catalogue() -> Value {
            json!({"tool":"tasks", "version":env!("CARGO_PKG_VERSION"), "messages":[
                $({"code":$code,"kind":$kind,"template":$text,"params":[$($param),*]}),*
            ]})
        }
    }
}
catalogue! {
    Created => ("tasks.created", "success", "Created the list \"{title}\".", ["title"]),
    Read => ("tasks.read", "success", "Tasks in the list: {tasks}.", ["tasks"]),
    Added => ("tasks.added", "success", "Added \"{title}\".", ["title"]),
    Edited => ("tasks.edited", "success", "Saved the changes.", []),
    Checked => ("tasks.checked", "success", "Updated the subtask.", []),
    Moved => ("tasks.moved", "success", "Moved the task to {group}.", ["group"]),
    Indented => ("tasks.indented", "success", "Turned the task into a subtask.", []),
    Outdented => ("tasks.outdented", "success", "Turned the subtask into a task.", []),
    Deleted => ("tasks.deleted", "success", "Deleted it from the list.", []),
    Renamed => ("tasks.renamed", "success", "Renamed the list to \"{title}\".", ["title"]),
    Snapshot => ("tasks.snapshot", "success", "The preview is ready.", []),
    TitleEmpty => ("tasks.title_empty", "error", "Enter a title (up to {limit} characters).", ["limit"]),
    TitleLong => ("tasks.title_too_long", "error", "Use at most {limit} characters for the title.", ["limit"]),
    NoteLong => ("tasks.note_too_long", "error", "Use at most {limit} characters for the note.", ["limit"]),
    Subtasks => ("tasks.too_many_subtasks", "error", "A task can have at most {limit} subtasks.", ["limit"]),
    Tasks => ("tasks.too_many_tasks", "error", "A list can have at most {limit} tasks.", ["limit"]),
    Control => ("tasks.control_character", "error", "Remove control characters; the text limit is {limit}.", ["limit"]),
    Conflict => ("tasks.conflict", "error", "The list changed: expected revision {expected}, current revision {actual}.", ["expected", "actual"]),
    SubtaskNote => ("tasks.subtask_note", "error", "Subtasks cannot have notes.", []),
    NotSubtask => ("tasks.not_subtask", "error", "Choose a subtask to mark as complete.", []),
    Index => ("tasks.index_out_of_range", "error", "Choose a position within the list.", []),
    Above => ("tasks.no_task_above", "error", "There is no task above this one.", []),
    HasSubtasks => ("tasks.has_subtasks", "error", "Move the subtasks first.", []),
    HasNote => ("tasks.has_note", "error", "Clear the note first.", []),
    Dropped => ("tasks.done_flag_dropped", "warning", "The promoted task is no longer marked complete.", []),
    NotFound => ("tasks.not_found", "error", "No task or subtask has the identifier {id}.", ["id"]),
    Validation => ("tasks.validation", "error", "Check the value in {field}.", ["field"]),
    Enum => ("tasks.enum", "error", "Choose one of {allowed} for {field}.", ["field", "allowed"]),
    Format => ("tasks.format", "error", "This is not a supported task list.", []),
    Ids => ("tasks.invalid_ids", "error", "Task identifiers must be unique and below the next identifier.", []),
    Groups => ("tasks.groups", "error", "Include all six groups in their usual order.", []),
    Exhausted => ("tasks.counter_exhausted", "error", "This list has reached its revision or identifier limit.", []),
    Json => ("tasks.invalid_json", "error", "Enter valid JSON.", []),
    Path => ("tasks.invalid_path", "error", "Use a relative file name inside the working folder.", []),
    Symlink => ("tasks.symlink", "error", "Symbolic links are not allowed.", []),
    FileType => ("tasks.not_regular_file", "error", "Choose a regular file.", []),
    Size => ("tasks.input_too_large", "error", "The file exceeds the limit of {limit} bytes.", ["limit"]),
    Io => ("tasks.io", "error", "The file operation failed.", []),
    Utf8 => ("tasks.invalid_utf8", "error", "Use UTF-8 text.", []),
    Arguments => ("tasks.arguments", "error", "Pass one JSON request, or send it through standard input.", []),
    Clock => ("tasks.clock", "error", "The current time could not be read.", [])
}

pub fn render(code: MessageCode, params: &Value, _locale: &str) -> String {
    let mut text = code.template().to_owned();
    if let Some(values) = params.as_object() {
        for (key, value) in values {
            let value = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            text = text.replace(&format!("{{{key}}}"), &value);
        }
    }
    text
}

#[derive(Clone, Debug, Serialize)]
pub struct Warning {
    pub code: &'static str,
    pub message: String,
    pub params: Value,
}
impl Warning {
    pub fn new(code: MessageCode, params: Value, locale: &str) -> Self {
        Self {
            code: code.code(),
            message: render(code, &params, locale),
            params,
        }
    }
}
