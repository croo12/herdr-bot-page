use anyhow::Result;
use serde_json::Value;

/// Keep the original response for callers of bot.connect, including provider-specific fields.
#[derive(Clone)]
pub(crate) struct HerdrAgent(Value);

impl HerdrAgent {
    pub(super) fn new(response: Value) -> Self {
        Self(response)
    }
    pub fn into_response(self) -> Value {
        self.0
    }
    pub fn name(&self) -> Option<&str> {
        self.0["name"].as_str()
    }
    pub fn pane_id(&self) -> Option<&str> {
        self.0["pane_id"].as_str()
    }
    pub fn required_pane(&self) -> Result<&str> {
        crate::domain::need(&self.0, "pane_id")
    }
    pub fn provider(&self) -> Option<&str> {
        self.0["agent"].as_str()
    }
    pub fn status(&self) -> Option<&str> {
        self.0["agent_status"].as_str()
    }
    pub fn cwd(&self) -> Option<&str> {
        self.0["cwd"].as_str()
    }
}
