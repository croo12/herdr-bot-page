use super::repository;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{
    io,
    path::{Path, PathBuf},
};
pub(crate) const MAX: u64 = 25 * 1024 * 1024;
fn file_name(raw: &str) -> String {
    let name = raw.rsplit(['/', '\\']).next().unwrap_or("attachment");
    let clean: String = name
        .chars()
        .filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c))
        .take(120)
        .collect();
    let clean = clean.trim_matches(['.', ' ']);
    if clean.is_empty() {
        "attachment".into()
    } else {
        clean.into()
    }
}
pub(super) fn stage(home: &Path, name: &str, bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return json!({"ok":false,"reason":"empty"});
    }
    if bytes.len() as u64 > MAX {
        return json!({"ok":false,"reason":"too-large"});
    }
    match repository::stage(home, &file_name(name), bytes) {
        Ok(path) => json!({"ok":true,"path":path}),
        Err(_) => json!({"ok":false,"reason":"failed"}),
    }
}
pub(super) fn access(
    home: &Path,
    action: &str,
    raw_path: &str,
    limit: Option<u64>,
    name: Option<&str>,
    choose_destination: impl FnOnce(&str) -> Option<PathBuf>,
) -> io::Result<Value> {
    let Some(path) = repository::source(home, raw_path) else {
        return Ok(Value::Null);
    };
    Ok(match action {
        "discard" => {
            repository::discard(&path)?;
            Value::Null
        }
        "media" => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let kind = mime.type_().as_str();
            if matches!(kind, "image" | "audio" | "video") {
                repository::data_url(&path, MAX)
                    .map_or(Value::Null, |url| json!({"kind":kind,"dataUrl":url}))
            } else {
                Value::Null
            }
        }
        "text" => {
            let (size, data) = repository::read_prefix(&path, 1024 * 1024)?;
            if data.iter().take(8000).any(|b| *b == 0) {
                json!({"kind":"binary","bytes":size})
            } else {
                json!({"kind":"text","text":String::from_utf8_lossy(&data),"bytes":size,"truncated":size>data.len() as u64})
            }
        }
        "bytes" => match repository::read_bounded(&path, limit.unwrap_or(MAX).min(MAX))? {
            repository::FileBytes::TooLarge(size) => json!({"kind":"too-large","size":size}),
            repository::FileBytes::Data(data) => {
                json!({"kind":"bytes","base64":STANDARD.encode(data)})
            }
        },
        "download" => {
            let name = name
                .or_else(|| path.file_name()?.to_str())
                .unwrap_or("attachment");
            if let Some(dest) = choose_destination(&file_name(name)) {
                repository::copy_to(&path, &dest)?;
                json!(true)
            } else {
                json!(false)
            }
        }
        _ => Value::Null,
    })
}
