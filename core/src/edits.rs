use crate::{
    Document, Error, GroupKey, MAX_SUBTASKS, MAX_TASKS, Timestamp, err, limit,
    messages::{MessageCode as C, Warning},
    model::{Subtask, Task, note, title},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;

/// Missing means unchanged; explicit null clears a note.
#[derive(Clone, Debug, Default)]
pub enum NotePatch {
    #[default]
    Keep,
    Clear,
    Set(String),
}
impl NotePatch {
    pub fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}
impl<'de> Deserialize<'de> for NotePatch {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<String>::deserialize(d)? {
            Some(s) => Self::Set(s),
            None => Self::Clear,
        })
    }
}
impl Serialize for NotePatch {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Set(v) => s.serialize_str(v),
            _ => s.serialize_none(),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum Edit {
    Add {
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
        expected_revision: u64,
        id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "NotePatch::is_keep")]
        note: NotePatch,
    },
    Check {
        expected_revision: u64,
        id: String,
        done: bool,
    },
    Move {
        expected_revision: u64,
        id: String,
        group: GroupKey,
        index: usize,
    },
    Indent {
        expected_revision: u64,
        id: String,
    },
    Outdent {
        expected_revision: u64,
        id: String,
    },
    Delete {
        expected_revision: u64,
        id: String,
    },
    Rename {
        expected_revision: u64,
        title: String,
    },
}
impl Edit {
    pub fn expected_revision(&self) -> u64 {
        match self {
            Self::Add {
                expected_revision, ..
            }
            | Self::Edit {
                expected_revision, ..
            }
            | Self::Check {
                expected_revision, ..
            }
            | Self::Move {
                expected_revision, ..
            }
            | Self::Indent {
                expected_revision, ..
            }
            | Self::Outdent {
                expected_revision, ..
            }
            | Self::Delete {
                expected_revision, ..
            }
            | Self::Rename {
                expected_revision, ..
            } => *expected_revision,
        }
    }
}
#[derive(Clone, Copy)]
struct Location {
    group: usize,
    task: usize,
    sub: Option<usize>,
}
fn locate(doc: &Document, id: &str) -> Result<Location, Error> {
    for (g, group) in doc.0.groups.iter().enumerate() {
        for (t, task) in group.tasks.iter().enumerate() {
            if task.id == id {
                return Ok(Location {
                    group: g,
                    task: t,
                    sub: None,
                });
            }
            if let Some(s) = task.subtasks.iter().position(|s| s.id == id) {
                return Ok(Location {
                    group: g,
                    task: t,
                    sub: Some(s),
                });
            }
        }
    }
    Err(Error::new(C::NotFound, json!({"id":id})))
}
fn task_count(doc: &Document) -> usize {
    doc.0.groups.iter().map(|g| g.tasks.len()).sum()
}
fn room_tasks(doc: &Document) -> Result<(), Error> {
    if task_count(doc) >= MAX_TASKS {
        Err(limit(C::Tasks, MAX_TASKS))
    } else {
        Ok(())
    }
}
fn room_subs(task: &Task) -> Result<(), Error> {
    if task.subtasks.len() >= MAX_SUBTASKS {
        Err(limit(C::Subtasks, MAX_SUBTASKS))
    } else {
        Ok(())
    }
}
fn position(index: Option<usize>, length: usize) -> Result<usize, Error> {
    let index = index.unwrap_or(length);
    if index > length {
        Err(err(C::Index))
    } else {
        Ok(index)
    }
}
fn next_id(doc: &mut Document) -> Result<String, Error> {
    let id = format!("t{}", doc.0.next_id);
    doc.0.next_id = doc
        .0
        .next_id
        .checked_add(1)
        .ok_or_else(|| err(C::Exhausted))?;
    Ok(id)
}

pub fn apply(doc: &Document, edit: &Edit, now: Timestamp) -> Result<Document, Error> {
    apply_with_warnings(doc, edit, now, "en").map(|(doc, _)| doc)
}
pub fn apply_with_warnings(
    doc: &Document,
    edit: &Edit,
    now: Timestamp,
    locale: &str,
) -> Result<(Document, Vec<Warning>), Error> {
    if edit.expected_revision() != doc.0.revision {
        return Err(Error::new(
            C::Conflict,
            json!({"expected":edit.expected_revision(),"actual":doc.0.revision}),
        ));
    }
    let revision = doc
        .0
        .revision
        .checked_add(1)
        .ok_or_else(|| err(C::Exhausted))?;
    let mut doc = doc.clone();
    let mut warnings = Vec::new();
    match edit {
        Edit::Add {
            group,
            title: name,
            note: n,
            index,
            parent,
            ..
        } => {
            let name = title(name)?;
            if let Some(parent) = parent {
                let loc = locate(&doc, parent)?;
                if loc.sub.is_some() {
                    return Err(err(C::NotSubtask));
                }
                if n.is_some() {
                    return Err(err(C::SubtaskNote));
                }
                let task = &doc.0.groups[loc.group].tasks[loc.task];
                room_subs(task)?;
                let index = position(*index, task.subtasks.len())?;
                let id = next_id(&mut doc)?;
                let task = &mut doc.0.groups[loc.group].tasks[loc.task];
                task.subtasks.insert(
                    index,
                    Subtask {
                        id,
                        title: name,
                        done: false,
                    },
                );
                task.updated_at = now.clone();
            } else {
                room_tasks(&doc)?;
                let g = group
                    .ok_or_else(|| crate::forms::invalid("/group"))?
                    .index();
                let index = position(*index, doc.0.groups[g].tasks.len())?;
                let n = n.as_deref().map(note).transpose()?;
                let id = next_id(&mut doc)?;
                doc.0.groups[g].tasks.insert(
                    index,
                    Task {
                        id,
                        title: name,
                        note: n,
                        created_at: now.clone(),
                        updated_at: now.clone(),
                        subtasks: Vec::new(),
                    },
                );
            }
        }
        Edit::Edit {
            id,
            title: name,
            note: patch,
            ..
        } => {
            let loc = locate(&doc, id)?;
            let name = name.as_deref().map(title).transpose()?;
            let task = &mut doc.0.groups[loc.group].tasks[loc.task];
            if let Some(s) = loc.sub {
                if !patch.is_keep() {
                    return Err(err(C::SubtaskNote));
                }
                if let Some(name) = name {
                    task.subtasks[s].title = name;
                }
            } else {
                if let Some(name) = name {
                    task.title = name;
                }
                match patch {
                    NotePatch::Keep => (),
                    NotePatch::Clear => task.note = None,
                    NotePatch::Set(v) => task.note = Some(note(v)?),
                }
            }
            task.updated_at = now.clone();
        }
        Edit::Check { id, done, .. } => {
            let loc = locate(&doc, id)?;
            let s = loc.sub.ok_or_else(|| err(C::NotSubtask))?;
            let task = &mut doc.0.groups[loc.group].tasks[loc.task];
            task.subtasks[s].done = *done;
            task.updated_at = now.clone();
        }
        Edit::Move {
            id, group, index, ..
        } => {
            let loc = locate(&doc, id)?;
            if loc.sub.is_some() {
                return Err(err(C::NotSubtask));
            }
            let g = group.index();
            let target_length = doc.0.groups[g].tasks.len() - usize::from(g == loc.group);
            let index = position(Some(*index), target_length)?;
            let mut task = doc.0.groups[loc.group].tasks.remove(loc.task);
            task.updated_at = now.clone();
            doc.0.groups[g].tasks.insert(index, task);
        }
        Edit::Indent { id, .. } => {
            let loc = locate(&doc, id)?;
            if loc.sub.is_some() {
                return Err(err(C::NotSubtask));
            }
            if loc.task == 0 {
                return Err(err(C::Above));
            }
            let tasks = &mut doc.0.groups[loc.group].tasks;
            if !tasks[loc.task].subtasks.is_empty() {
                return Err(err(C::HasSubtasks));
            }
            if tasks[loc.task].note.is_some() {
                return Err(err(C::HasNote));
            }
            room_subs(&tasks[loc.task - 1])?;
            let task = tasks.remove(loc.task);
            let parent = &mut tasks[loc.task - 1];
            parent.subtasks.push(Subtask {
                id: task.id,
                title: task.title,
                done: false,
            });
            parent.updated_at = now.clone();
        }
        Edit::Outdent { id, .. } => {
            let loc = locate(&doc, id)?;
            let s = loc.sub.ok_or_else(|| err(C::NotSubtask))?;
            room_tasks(&doc)?;
            let tasks = &mut doc.0.groups[loc.group].tasks;
            let sub = tasks[loc.task].subtasks.remove(s);
            tasks[loc.task].updated_at = now.clone();
            if sub.done {
                warnings.push(Warning::new(C::Dropped, json!({}), locale));
            }
            tasks.insert(
                loc.task + 1,
                Task {
                    id: sub.id,
                    title: sub.title,
                    note: None,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    subtasks: Vec::new(),
                },
            );
        }
        Edit::Delete { id, .. } => {
            let loc = locate(&doc, id)?;
            let tasks = &mut doc.0.groups[loc.group].tasks;
            if let Some(s) = loc.sub {
                tasks[loc.task].subtasks.remove(s);
                tasks[loc.task].updated_at = now.clone();
            } else {
                tasks.remove(loc.task);
            }
        }
        Edit::Rename { title: name, .. } => doc.0.title = title(name)?,
    }
    doc.0.revision = revision;
    doc.0.updated_at = now;
    Ok((doc, warnings))
}
