use crate::{
    domain::patch,
    storage::{Repositories, Store},
};
use anyhow::Result;
use serde_json::{json, Value};

fn load(repos: &Repositories) -> Result<Value> {
    Ok(repos.settings.get()?.unwrap_or_else(|| {
        json!({
            "defaultWorkingFolder": null,
            "defaultProvider": null,
            "defaultPermissionMode": null,
            "terminal": null,
        })
    }))
}

pub(crate) fn get_settings(store: &Store) -> Result<Value> {
    store.tx(|repos, _| load(repos))
}
pub(crate) fn update_settings(store: &Store, params: &Value) -> Result<Value> {
    store.tx(|repos, _| {
        let mut value = load(repos)?;
        patch(
            &mut value,
            params,
            &[
                "defaultWorkingFolder",
                "defaultProvider",
                "defaultPermissionMode",
                "terminal",
            ],
        );
        repos.settings.save(&value)?;
        Ok(value)
    })
}
pub(crate) fn bot_defaults(store: &Store) -> Result<Value> {
    store.tx(|repos, _| defaults(repos))
}
pub(crate) fn defaults(repos: &Repositories) -> Result<Value> {
    let settings = load(repos)?;
    let last = repos
        .chats
        .list()?
        .into_iter()
        .rev()
        .find_map(|chat| chat["bot"]["cwd"].as_str().map(String::from));
    Ok(
        json!({"cwd":std::env::var("HERDR_BOT_DEFAULT_CWD").ok().or_else(||settings["defaultWorkingFolder"].as_str().map(String::from)).or(last).unwrap_or_default(),
        "provider":settings["defaultProvider"].as_str().map(String::from).or_else(||std::env::var("HERDR_BOT_DEFAULT_KIND").ok()).unwrap_or("claude".into()),
        "permissionMode":settings["defaultPermissionMode"].as_str().unwrap_or("ask"),"homeDir":dirs::home_dir()}),
    )
}
