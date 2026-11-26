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
