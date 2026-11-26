use anyhow::{anyhow, Result};
use serde_json::Value as V;
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
pub(crate) fn s<'a>(v: &'a V, k: &str) -> &'a str {
    v[k].as_str().unwrap_or_default()
}
pub(crate) fn need<'a>(v: &'a V, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .filter(|x| !x.trim().is_empty())
        .ok_or_else(|| anyhow!("invalid_args: missing {k}"))
}
pub(crate) fn strings(v: &V) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(String::from))
        .collect()
}
pub(crate) fn patch(to: &mut V, from: &V, keys: &[&str]) {
    for k in keys {
        if let Some(v) = from.get(k) {
            to[k] = v.clone();
        }
    }
}
