use crate::{
    domain::{id, need, string_field},
    infrastructure::paths::expand,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};

const KINDS: &[&str] = &[
    "pi",
    "claude",
    "codex",
    "gemini",
    "cursor",
    "devin",
    "agy",
    "cline",
    "omp",
    "mastracode",
    "opencode",
    "copilot",
    "kimi",
    "kiro",
    "droid",
    "amp",
    "grok",
    "hermes",
    "kilo",
    "qodercli",
    "qwen",
    "maki",
    "muse",
];
pub(super) fn bot_id(params: &Value) -> Result<String> {
    if let Some(id) = params["id"].as_str() {
        if !id.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            || id.len() > 32
            || id.starts_with("room-")
            || !id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
            })
        {
            bail!("invalid_bot_id: {id}")
        }
        return Ok(id.into());
    }
    let slug: String = string_field(params, "name")
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .take(20)
        .collect();
    let slug = slug.trim_matches('-');
    let prefix = if slug.starts_with(|character: char| character.is_ascii_lowercase())
        && !slug.starts_with("room-")
    {
        slug
    } else {
        "bot"
    };
    Ok(format!("{prefix}-{}", &id()[..6]))
}
pub(super) fn validate(bot: &mut Value, allow_home: bool, require_choices: bool) -> Result<()> {
    let kind = need(bot, "provider")?;
    if !KINDS.contains(&kind) {
        bail!("unsupported_kind: {kind}")
    }
    if !matches!(string_field(bot, "permissionMode"), "ask" | "auto") {
        bail!("invalid_launch_options: invalid permission mode")
    }
    let reasoning = matches!(kind, "codex" | "claude" | "grok");
    let model = reasoning || matches!(kind, "gemini" | "agy" | "opencode");
    for (field, can) in [("model", model), ("reasoningEffort", reasoning)] {
        let selected = !string_field(bot, field).trim().is_empty();
        if (selected && !can) || (require_choices && can && !selected) {
            bail!("invalid_launch_options: check {field} for {kind}")
        }
    }
    if bot["reasoningEffort"].is_string()
        && !["low", "medium", "high", "xhigh", "max"]
            .contains(&string_field(bot, "reasoningEffort"))
    {
        bail!("invalid_launch_options: unknown reasoning effort")
    }
    if string_field(bot, "cwd").is_empty() {
        bail!("missing_cwd: choose a working folder in Settings or Advanced setup")
    }
    let cwd = expand(string_field(bot, "cwd"));
    if !cwd.is_dir() {
        bail!("missing_cwd: {}", cwd.display())
    }
    if !allow_home
        && cwd.canonicalize().ok() == dirs::home_dir().and_then(|home| home.canonicalize().ok())
    {
        bail!("home_cwd_requires_confirmation: confirm the home folder")
    }
    bot["cwd"] = json!(cwd);
    Ok(())
}
