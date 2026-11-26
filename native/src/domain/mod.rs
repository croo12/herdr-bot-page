mod turn;
mod value;
use serde_json::Value;
pub(crate) use turn::{ChatJob, Turn};
pub(crate) use value::{id, need, now, patch, string_field, strings};

pub(crate) type Events = Vec<(&'static str, Value)>;
