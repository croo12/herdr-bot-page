use anyhow::{anyhow, Result};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub(crate) fn id() -> String {
    Uuid::new_v4().to_string()
}
pub(crate) fn string_field<'value>(value: &'value Value, key: &str) -> &'value str {
    value[key].as_str().unwrap_or_default()
}
pub(crate) fn need<'value>(value: &'value Value, key: &str) -> Result<&'value str> {
    value[key]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| anyhow!("invalid_args: missing {key}"))
}
pub(crate) fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(String::from))
        .collect()
}
pub(crate) fn patch(to: &mut Value, from: &Value, keys: &[&str]) {
    for key in keys {
        if let Some(value) = from.get(key) {
            to[key] = value.clone();
        }
    }
}
