use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct BotId(String);

impl From<&str> for BotId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl BotId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Bot {
    pub id: BotId,
    pub name: String,
    pub label: String,
    pub instruction: String,
    pub avatar_color: Option<AvatarColor>,
    // An adopted agent may not have reported its working directory.
    pub working_directory: Option<PathBuf>,
    pub provider: Provider,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AvatarColor {
    Black,
    Brown,
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Violet,
    Magenta,
    #[serde(other)]
    Gray,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Provider {
    Claude {
        model: Option<ClaudeModelId>,
        reasoning_effort: Option<ReasoningEffort>,
        permission_mode: ClaudePermissionMode,
    },
    Codex {
        model: Option<CodexModelId>,
        reasoning_effort: Option<ReasoningEffort>,
        permission_mode: CodexPermissionMode,
    },
    Grok {
        model: Option<GrokModelId>,
        reasoning_effort: Option<ReasoningEffort>,
    },
    Gemini {
        model: Option<GeminiModelId>,
    },
    Antigravity {
        model: Option<AntigravityModelId>,
    },
    OpenCode {
        model: Option<OpenCodeModelId>,
    },
    Other(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ClaudeModelId(pub String);
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CodexModelId(pub String);
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct GrokModelId(pub String);
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct GeminiModelId(pub String);
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AntigravityModelId(pub String);
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct OpenCodeModelId(pub String);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ReasoningEffort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClaudePermissionMode {
    Default,
    BypassPermissions,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CodexPermissionMode {
    Default,
    WorkspaceWriteOnRequest,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaunchSettings {
    pub cwd: Option<PathBuf>,
    pub provider: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub permission_mode: Option<PermissionMode>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PermissionMode {
    Ask,
    Auto,
}

impl From<&LaunchSettings> for Provider {
    fn from(settings: &LaunchSettings) -> Self {
        let auto = matches!(settings.permission_mode, Some(PermissionMode::Auto));
        let model = settings.model.clone();
        match settings.provider.as_str() {
            "claude" => Provider::Claude {
                model: model.map(ClaudeModelId),
                reasoning_effort: settings.reasoning_effort.clone(),
                permission_mode: if auto {
                    ClaudePermissionMode::BypassPermissions
                } else {
                    ClaudePermissionMode::Default
                },
            },
            "codex" => Provider::Codex {
                model: model.map(CodexModelId),
                reasoning_effort: settings.reasoning_effort.clone(),
                permission_mode: if auto {
                    CodexPermissionMode::WorkspaceWriteOnRequest
                } else {
                    CodexPermissionMode::Default
                },
            },
            "grok" => Provider::Grok {
                model: model.map(GrokModelId),
                reasoning_effort: settings.reasoning_effort.clone(),
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
            _ => Provider::Other(settings.provider.clone()),
        }
    }
}
