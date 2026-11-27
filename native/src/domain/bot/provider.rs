use super::model::*;
use anyhow::{bail, Result};
use serde::Deserialize;

/// Provider fields as sent over IPC or stored in an existing bot record.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderDraft {
    pub provider: String,
    #[serde(flatten)]
    pub options: ProviderOptions,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderOptions {
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub permission_mode: Option<PermissionMode>,
}

#[derive(PartialEq, Eq)]
pub(crate) enum SelectionFor {
    NewBot,
    Relaunch,
}

impl ProviderDraft {
    pub fn select(
        self,
        usage: SelectionFor,
        default_permission: PermissionMode,
    ) -> Result<Provider> {
        let model_selected = self
            .options
            .model
            .as_ref()
            .is_some_and(|model| !model.trim().is_empty());
        let effort_selected = self.options.reasoning_effort.is_some();
        let provider = self.existing(Some(default_permission));
        let name = provider.name();
        let (supports_model, supports_effort) = match &provider {
            Provider::Claude { .. } | Provider::Codex { .. } | Provider::Grok { .. } => {
                (true, true)
            }
            Provider::Gemini { .. } | Provider::Antigravity { .. } | Provider::OpenCode { .. } => {
                (true, false)
            }
            Provider::Other(kind) => match kind.as_str() {
                "pi" | "cursor" | "devin" | "cline" | "omp" | "mastracode" | "copilot" | "kimi"
                | "kiro" | "droid" | "amp" | "hermes" | "kilo" | "qodercli" | "qwen" | "maki"
                | "muse" => (false, false),
                _ => bail!("unsupported_kind: {name}"),
            },
        };
        for (field, supported, selected) in [
            ("model", supports_model, model_selected),
            ("reasoningEffort", supports_effort, effort_selected),
        ] {
            if (selected && !supported) || (usage == SelectionFor::NewBot && supported && !selected)
            {
                bail!("invalid_launch_options: check {field} for {name}");
            }
        }
        Ok(provider)
    }

    /// An existing external agent may not report its model or reasoning effort.
    pub fn existing(self, default_permission: Option<PermissionMode>) -> Provider {
        let permission = self.options.permission_mode.or(default_permission);
        let model = self.options.model;
        let reasoning_effort = self.options.reasoning_effort;
        match self.provider.as_str() {
            "claude" => Provider::Claude {
                model: model.map(ClaudeModelId),
                reasoning_effort,
                permission_mode: match permission {
                    None => ClaudePermissionMode::Inherit,
                    Some(PermissionMode::Ask) => ClaudePermissionMode::Default,
                    Some(PermissionMode::Auto) => ClaudePermissionMode::BypassPermissions,
                },
            },
            "codex" => Provider::Codex {
                model: model.map(CodexModelId),
                reasoning_effort,
                permission_mode: match permission {
                    None => CodexPermissionMode::Inherit,
                    Some(PermissionMode::Ask) => CodexPermissionMode::Default,
                    Some(PermissionMode::Auto) => CodexPermissionMode::WorkspaceWriteOnRequest,
                },
            },
            "grok" => Provider::Grok {
                model: model.map(GrokModelId),
                reasoning_effort,
            },
            "gemini" => Provider::Gemini {
                model: model.map(GeminiModelId),
            },
            "agy" => Provider::Antigravity {
                model: model.map(AntigravityModelId),
            },
            "opencode" => Provider::OpenCode {
                model: model.map(OpenCodeModelId),
            },
            _ => Provider::Other(self.provider),
        }
    }
}

impl Provider {
    pub fn options(&self) -> ProviderOptions {
        let (model, effort, permission) = match self {
            Self::Claude {
                model,
                reasoning_effort,
                permission_mode,
            } => (
                model.as_ref().map(|model| &model.0),
                reasoning_effort.as_ref(),
                match permission_mode {
                    ClaudePermissionMode::Inherit => None,
                    ClaudePermissionMode::Default => Some(PermissionMode::Ask),
                    ClaudePermissionMode::BypassPermissions => Some(PermissionMode::Auto),
                },
            ),
            Self::Codex {
                model,
                reasoning_effort,
                permission_mode,
            } => (
                model.as_ref().map(|model| &model.0),
                reasoning_effort.as_ref(),
                match permission_mode {
                    CodexPermissionMode::Inherit => None,
                    CodexPermissionMode::Default => Some(PermissionMode::Ask),
                    CodexPermissionMode::WorkspaceWriteOnRequest => Some(PermissionMode::Auto),
                },
            ),
            Self::Grok {
                model,
                reasoning_effort,
            } => (
                model.as_ref().map(|model| &model.0),
                reasoning_effort.as_ref(),
                None,
            ),
            Self::Gemini { model } => (model.as_ref().map(|model| &model.0), None, None),
            Self::Antigravity { model } => (model.as_ref().map(|model| &model.0), None, None),
            Self::OpenCode { model } => (model.as_ref().map(|model| &model.0), None, None),
            Self::Other(_) => (None, None, None),
        };
        ProviderOptions {
            model: model.cloned(),
            reasoning_effort: effort.cloned(),
            permission_mode: permission,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn saved_settings_round_trip_only_the_fields_the_provider_uses() -> Result<()> {
        for (name, model, effort, permission) in [
            (
                "claude",
                Some("custom"),
                Some(ReasoningEffort::High),
                Some(PermissionMode::Auto),
            ),
            (
                "codex",
                Some("custom"),
                Some(ReasoningEffort::High),
                Some(PermissionMode::Auto),
            ),
            ("grok", Some("custom"), Some(ReasoningEffort::High), None),
            ("gemini", Some("custom"), None, None),
            ("agy", Some("custom"), None, None),
            ("opencode", Some("custom"), None, None),
            ("pi", None, None, None),
        ] {
            let draft: ProviderDraft = serde_json::from_value(json!({
                "provider": name, "model": "custom", "reasoningEffort": "high", "permissionMode": "auto",
            }))?;
            let provider = draft.existing(None);
            let options = provider.options();
            assert_eq!(options.model.as_deref(), model);
            assert_eq!(options.reasoning_effort, effort);
            assert_eq!(options.permission_mode, permission);
            assert_eq!(
                ProviderDraft {
                    provider: name.into(),
                    options
                }
                .existing(None),
                provider
            );
        }
        for name in ["claude", "codex"] {
            let legacy: ProviderDraft = serde_json::from_value(json!({"provider":name}))?;
            assert_eq!(legacy.existing(None).options().permission_mode, None);
        }
        Ok(())
    }
}
