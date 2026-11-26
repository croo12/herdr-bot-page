use anyhow::Result;
use serde_json::{json, Value as V};
use std::{env, path::PathBuf};
pub fn default_home() -> PathBuf {
    if let Some(home) = env::var_os("HERDR_BOT_HOME") {
        return PathBuf::from(home);
    }
    if let Ok(exe) = env::current_exe() {
        if exe.file_stem().is_some_and(|s| s == "herdr-bot")
            && exe.parent().is_some_and(|p| {
                p.ends_with("bin") && p.parent().is_some_and(|h| h.join("chat.sqlite3").exists())
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
pub(crate) fn directory(raw: &str) -> Result<V> {
    let path = expand(raw.trim());
    let exists = path.is_dir();
    let (parent, prefix) = if raw.is_empty() || raw.ends_with(['/', '\\']) {
        (path.as_path(), "")
    } else {
        (
            path.parent().unwrap_or(&path),
            path.file_name().and_then(|x| x.to_str()).unwrap_or(""),
        )
    };
    let mut entries = vec![];
    if let Ok(rd) = parent.read_dir() {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir()
                && (!name.starts_with('.') || prefix.starts_with('.'))
                && name.to_lowercase().starts_with(&prefix.to_lowercase())
            {
                entries.push(e.path());
            }
        }
    }
    entries.sort();
    entries.truncate(50);
    Ok(
        json!({"exists":exists,"isHome":exists&&path.canonicalize().ok()==dirs::home_dir().and_then(|x|x.canonicalize().ok()),"entries":entries}),
    )
}
