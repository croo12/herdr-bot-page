mod value;
use serde_json::Value as V;
pub(crate) use value::{id, need, now, patch, s, strings};

pub(crate) type Events = Vec<(&'static str, V)>;
