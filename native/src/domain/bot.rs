use serde::Deserialize;
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

#[derive(Debug, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Deserialize, PartialEq, Eq)]
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
