use anyhow::Result;
use serde_json::{json, Value};
use std::{env, path::PathBuf};
pub fn default_home() -> PathBuf {
    if let Some(home) = env::var_os("HERDR_BOT_HOME") {
        return PathBuf::from(home);
    }
    if let Ok(exe) = env::current_exe() {
        if exe.file_stem().is_some_and(|stem| stem == "herdr-bot")
            && exe.parent().is_some_and(|directory| {
                directory.ends_with("bin")
                    && directory
                        .parent()
                        .is_some_and(|home| home.join("chat.sqlite3").exists())
            })
        {
            return exe.parent().unwrap().parent().unwrap().into();
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".herdr-bot")
}
pub(crate) fn expand(path: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    if path == "~" || path.is_empty() {
        home
    } else if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        home.join(rest)
    } else {
        home.join(path)
    }
}
pub(crate) fn directory(raw: &str) -> Result<Value> {
    let path = expand(raw.trim());
    let exists = path.is_dir();
    let (parent, prefix) = if raw.is_empty() || raw.ends_with(['/', '\\']) {
        (path.as_path(), "")
    } else {
        (
            path.parent().unwrap_or(&path),
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
        )
    };
    let mut entries = vec![];
    if let Ok(rd) = parent.read_dir() {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir()
                && (!name.starts_with('.') || prefix.starts_with('.'))
                && name.to_lowercase().starts_with(&prefix.to_lowercase())
            {
                entries.push(entry.path());
            }
        }
    }
    entries.sort();
    entries.truncate(50);
    Ok(
        json!({"exists":exists,"isHome":exists&&path.canonicalize().ok()==dirs::home_dir().and_then(|home|home.canonicalize().ok()),"entries":entries}),
    )
}
