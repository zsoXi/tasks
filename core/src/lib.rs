//! In-memory task lists. No filesystem, clock or network access.
mod edits;
pub mod forms;
pub mod messages;
mod model;
mod snapshot;
pub use edits::*;
use messages::MessageCode;
pub use model::*;
use serde_json::{Value, json};
pub use snapshot::*;

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_NOTE_CHARS: usize = 2_000;
pub const MAX_SUBTASKS: usize = 50;
pub const MAX_TASKS: usize = 500;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub code: MessageCode,
    pub params: Value,
    message: String,
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
impl Error {
    pub fn new(code: MessageCode, params: Value) -> Self {
        Self {
            code,
            message: messages::render(code, &params, "en"),
            params,
            source: None,
        }
    }
    pub fn caused(
        code: MessageCode,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        let params = if code == MessageCode::Validation {
            json!({"field":""})
        } else {
            json!({})
        };
        let mut error = Self::new(code, params);
        error.source = Some(Box::new(source));
        error
    }
}
pub(crate) fn err(code: MessageCode) -> Error {
    Error::new(code, json!({}))
}
pub(crate) fn limit(code: MessageCode, limit: usize) -> Error {
    Error::new(code, json!({"limit":limit}))
}
pub fn schema() -> Value {
    forms::schema()
}
