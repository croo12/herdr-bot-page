use crate::{domain::patch, storage::Repositories, Engine};
use anyhow::Result;
use serde_json::{json, Value as V};

fn load(repos: &Repositories) -> Result<V> {
    Ok(repos.settings.get()?.unwrap_or_else(|| {
        json!({
            "defaultWorkingFolder": null,
            "defaultProvider": null,
            "defaultPermissionMode": null,
            "terminal": null,
        })
    }))
}

pub(crate) fn get_settings(engine: &Engine) -> Result<V> {
    engine.tx(|repos, _| load(repos))
}
pub(crate) fn update_settings(engine: &Engine, p: &V) -> Result<V> {
    engine.tx(|c, _| {
        let mut value = load(c)?;
        patch(
            &mut value,
            p,
            &[
                "defaultWorkingFolder",
                "defaultProvider",
                "defaultPermissionMode",
                "terminal",
            ],
        );
        c.settings.save(&value)?;
        Ok(value)
    })
}
pub(crate) fn bot_defaults(engine: &Engine) -> Result<V> {
    engine.tx(|c, _| defaults(c))
}
pub(crate) fn defaults(c: &Repositories) -> Result<V> {
    let v = load(c)?;
    let last = c
        .chats
        .list()?
        .into_iter()
        .rev()
        .find_map(|v| v["bot"]["cwd"].as_str().map(String::from));
    Ok(
        json!({"cwd":std::env::var("HERDR_BOT_DEFAULT_CWD").ok().or_else(||v["defaultWorkingFolder"].as_str().map(String::from)).or(last).unwrap_or_default(),
        "provider":v["defaultProvider"].as_str().map(String::from).or_else(||std::env::var("HERDR_BOT_DEFAULT_KIND").ok()).unwrap_or("claude".into()),
        "permissionMode":v["defaultPermissionMode"].as_str().unwrap_or("ask"),"homeDir":dirs::home_dir()}),
    )
}
