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

#[derive(Clone, Deserialize)]
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
        let provider = self.existing(default_permission);
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
    pub fn existing(self, default_permission: PermissionMode) -> Provider {
        let permission = self.options.permission_mode.unwrap_or(default_permission);
        let model = self.options.model;
        let reasoning_effort = self.options.reasoning_effort;
        match self.provider.as_str() {
            "claude" => Provider::Claude {
                model: model.map(ClaudeModelId),
                reasoning_effort,
                permission_mode: match permission {
                    PermissionMode::Ask => ClaudePermissionMode::Default,
                    PermissionMode::Auto => ClaudePermissionMode::BypassPermissions,
                },
            },
            "codex" => Provider::Codex {
                model: model.map(CodexModelId),
                reasoning_effort,
                permission_mode: match permission {
                    PermissionMode::Ask => CodexPermissionMode::Default,
                    PermissionMode::Auto => CodexPermissionMode::WorkspaceWriteOnRequest,
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
