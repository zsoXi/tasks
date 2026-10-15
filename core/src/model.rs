use crate::{
    Error, MAX_NOTE_CHARS, MAX_SUBTASKS, MAX_TASKS, MAX_TITLE_CHARS, err, limit,
    messages::MessageCode as C,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

/// UTC calendar time, with whole seconds and a literal Z suffix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Timestamp(String);
impl Timestamp {
    pub fn new(value: &str) -> Result<Self, Error> {
        let b = value.as_bytes();
        let number =
            |start: usize, end: usize| value.get(start..end).and_then(|s| s.parse::<u32>().ok());
        let valid = b.len() == 20
            && b[4] == b'-'
            && b[7] == b'-'
            && b[10] == b'T'
            && b[13] == b':'
            && b[16] == b':'
            && b[19] == b'Z'
            && b.iter()
                .enumerate()
                .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19) || c.is_ascii_digit());
        if !valid {
            return Err(crate::forms::invalid(""));
        }
        let year = number(0, 4).ok_or_else(|| crate::forms::invalid(""))?;
        let month = number(5, 7).ok_or_else(|| crate::forms::invalid(""))?;
        let day = number(8, 10).ok_or_else(|| crate::forms::invalid(""))?;
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => 0,
        };
        if year == 0
            || day == 0
            || day > days
            || number(11, 13).is_none_or(|v| v > 23)
            || number(14, 16).is_none_or(|v| v > 59)
            || number(17, 19).is_none_or(|v| v > 59)
        {
            return Err(crate::forms::invalid(""));
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKey {
    Now,
    Later,
    Ongoing,
    Ideas,
    Done,
    Rejected,
}
impl GroupKey {
    pub const ALL: [Self; 6] = [
        Self::Now,
        Self::Later,
        Self::Ongoing,
        Self::Ideas,
        Self::Done,
        Self::Rejected,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Now => "now",
            Self::Later => "later",
            Self::Ongoing => "ongoing",
            Self::Ideas => "ideas",
            Self::Done => "done",
            Self::Rejected => "rejected",
        }
    }
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Now => 0,
            Self::Later => 1,
            Self::Ongoing => 2,
            Self::Ideas => 3,
            Self::Done => 4,
            Self::Rejected => 5,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Subtask {
    pub id: String,
    pub title: String,
    pub done: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Task {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub subtasks: Vec<Subtask>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Group {
    pub key: GroupKey,
    pub tasks: Vec<Task>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDocument {
    pub format: String,
    pub id: String,
    pub title: String,
    pub revision: u64,
    pub next_id: u64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub groups: Vec<Group>,
}
/// A validated list. Deserialize uses the same validation as parse.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct Document(pub(crate) RawDocument);
impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut raw = RawDocument::deserialize(d)?;
        validate(&mut raw).map_err(serde::de::Error::custom)?;
        Ok(Self(raw))
    }
}
impl Document {
    pub fn revision(&self) -> u64 {
        self.0.revision
    }
    pub fn id(&self) -> &str {
        &self.0.id
    }
}

pub(crate) fn title(value: &str) -> Result<String, Error> {
    let value = value.trim();
    if value.is_empty() {
        return Err(limit(C::TitleEmpty, MAX_TITLE_CHARS));
    }
    if value.chars().count() > MAX_TITLE_CHARS {
        return Err(limit(C::TitleLong, MAX_TITLE_CHARS));
    }
    if value.chars().any(char::is_control) {
        return Err(limit(C::Control, MAX_TITLE_CHARS));
    }
    Ok(value.to_owned())
}
pub(crate) fn note(value: &str) -> Result<String, Error> {
    if value.chars().count() > MAX_NOTE_CHARS {
        return Err(limit(C::NoteLong, MAX_NOTE_CHARS));
    }
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        return Err(limit(C::Control, MAX_NOTE_CHARS));
    }
    Ok(value.to_owned())
}
fn validate(raw: &mut RawDocument) -> Result<(), Error> {
    if raw.format != "tasks/1" {
        return Err(err(C::Format));
    }
    if raw.id.is_empty() || raw.id.chars().any(char::is_control) {
        return Err(crate::forms::invalid("/id"));
    }
    raw.title = title(&raw.title)?;
    if raw.groups.iter().map(|g| g.key).ne(GroupKey::ALL) {
        return Err(err(C::Groups));
    }
    if raw.groups.iter().map(|g| g.tasks.len()).sum::<usize>() > MAX_TASKS {
        return Err(limit(C::Tasks, MAX_TASKS));
    }
    let mut ids = HashSet::new();
    let mut check_id = |id: &str| -> Result<(), Error> {
        let n = id
            .strip_prefix('t')
            .and_then(|n| n.parse::<u64>().ok())
            .ok_or_else(|| err(C::Ids))?;
        if n == 0 || n >= raw.next_id || id != format!("t{n}") || !ids.insert(id.to_owned()) {
            return Err(err(C::Ids));
        }
        Ok(())
    };
    if raw.next_id == 0 {
        return Err(err(C::Ids));
    }
    for group in &mut raw.groups {
        for task in &mut group.tasks {
            check_id(&task.id)?;
            task.title = title(&task.title)?;
            if let Some(n) = &mut task.note {
                *n = note(n)?;
            }
            if task.subtasks.len() > MAX_SUBTASKS {
                return Err(limit(C::Subtasks, MAX_SUBTASKS));
            }
            for sub in &mut task.subtasks {
                check_id(&sub.id)?;
                sub.title = title(&sub.title)?;
            }
        }
    }
    Ok(())
}
pub fn parse(input: &str) -> Result<Document, Error> {
    let value: Value = serde_json::from_str(input).map_err(|e| Error::caused(C::Json, e))?;
    crate::forms::validate(&value, &crate::forms::document_schema(), "")?;
    let mut raw: RawDocument =
        serde_json::from_value(value).map_err(|e| Error::caused(C::Validation, e))?;
    validate(&mut raw)?;
    Ok(Document(raw))
}
/// Stable compact JSON. All fields have JSON-native, infallible representations.
pub fn to_json(doc: &Document) -> String {
    json!(doc).to_string()
}
pub fn new_list(id: &str, name: &str, now: Timestamp) -> Result<Document, Error> {
    let mut raw = RawDocument {
        format: "tasks/1".into(),
        id: id.into(),
        title: title(name)?,
        revision: 0,
        next_id: 1,
        created_at: now.clone(),
        updated_at: now,
        groups: GroupKey::ALL
            .into_iter()
            .map(|key| Group {
                key,
                tasks: Vec::new(),
            })
            .collect(),
    };
    validate(&mut raw)?;
    Ok(Document(raw))
}
pub fn view(doc: &Document) -> Value {
    json!({"list":{"id":doc.0.id,"title":doc.0.title,"revision":doc.0.revision},
        "groups":doc.0.groups.iter().map(|group| json!({"key":group.key,"tasks":group.tasks.iter().map(|task| {
            let mut v = json!({"id":task.id,"title":task.title,"subtasks":task.subtasks});
            if let Some(note) = &task.note { v["note"] = json!(note); }
            if !task.subtasks.is_empty() { v["progress"] = json!({"done":task.subtasks.iter().filter(|s| s.done).count(),"total":task.subtasks.len()}); }
            v
        }).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}
