use crate::{domain::bot::LaunchSettings, infrastructure::paths::expand};
use anyhow::{bail, Result};
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
pub(super) fn validate(
    settings: &mut LaunchSettings,
    allow_home: bool,
    require_choices: bool,
) -> Result<()> {
    let kind = settings.provider.as_str();
    if !KINDS.contains(&kind) {
        bail!("unsupported_kind: {kind}")
    }
    if settings.permission_mode.is_none() {
        bail!("invalid_launch_options: invalid permission mode")
    }
    let reasoning = matches!(kind, "codex" | "claude" | "grok");
    let model = reasoning || matches!(kind, "gemini" | "agy" | "opencode");
    for (field, can, selected) in [
        (
            "model",
            model,
            settings
                .model
                .as_ref()
                .is_some_and(|name| !name.trim().is_empty()),
        ),
        (
            "reasoningEffort",
            reasoning,
            settings.reasoning_effort.is_some(),
        ),
    ] {
        if (selected && !can) || (require_choices && can && !selected) {
            bail!("invalid_launch_options: check {field} for {kind}")
        }
    }
    let raw = settings
        .cwd
        .as_deref()
        .and_then(|path| path.to_str())
        .unwrap_or_default();
    if raw.is_empty() {
        bail!("missing_cwd: choose a working folder in Settings or Advanced setup")
    }
    let cwd = expand(raw);
    if !cwd.is_dir() {
        bail!("missing_cwd: {}", cwd.display())
    }
    if !allow_home
        && cwd.canonicalize().ok() == dirs::home_dir().and_then(|home| home.canonicalize().ok())
    {
        bail!("home_cwd_requires_confirmation: confirm the home folder")
    }
    settings.cwd = Some(cwd);
    Ok(())
}
