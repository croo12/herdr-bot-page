use crate::storage::Store;
use anyhow::Result;
use serde_json::{json, Value};

pub(crate) fn bot_defaults(store: &Store) -> Result<Value> {
    store.tx(|repos, _| {
        let settings = repos.settings.get()?.unwrap_or_default();
        let last_cwd = repos
            .chats
            .list()?
            .into_iter()
            .rev()
            .find_map(|chat| chat["bot"]["cwd"].as_str().map(String::from));
        let cwd = std::env::var("HERDR_BOT_DEFAULT_CWD")
            .ok()
            .or_else(|| settings["defaultWorkingFolder"].as_str().map(String::from))
            .or(last_cwd)
            .unwrap_or_default();
        let provider = settings["defaultProvider"]
            .as_str()
            .map(String::from)
            .or_else(|| std::env::var("HERDR_BOT_DEFAULT_KIND").ok())
            .unwrap_or_else(|| "claude".into());
        Ok(json!({
            "cwd": cwd,
            "provider": provider,
            "permissionMode": settings["defaultPermissionMode"].as_str().unwrap_or("ask"),
            "homeDir": dirs::home_dir()
        }))
    })
}
