use crate::{domain::need, modules::chats::ProfileUpdate};
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(super) fn input(params: &Value) -> Result<ProfileUpdate> {
    fn field<T: DeserializeOwned>(params: &Value, key: &str) -> Result<Option<T>> {
        params
            .get(key)
            .map(T::deserialize)
            .transpose()
            .with_context(|| format!("invalid_args: invalid {key}"))
    }
    Ok(ProfileUpdate {
        name: params
            .get("name")
            .map(|_| need(params, "name").map(str::to_owned))
            .transpose()?,
        label: field(params, "title")?,
        instruction: field(params, "description")?,
        avatar_color: field(params, "avatarColor")?,
        avatar_shape: field(params, "avatarShape")?,
    })
}
