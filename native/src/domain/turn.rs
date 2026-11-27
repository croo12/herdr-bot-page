use super::DelegationRef;

#[derive(Clone)]
pub(crate) struct Delivery {
    pub chat: String,
    pub bot: String,
    pub seq: i64,
}

#[derive(Clone, Default)]
pub(crate) struct TurnContext {
    pub message_id: Option<String>,
    pub delegation: Option<DelegationRef>,
}

impl TurnContext {
    pub(crate) fn from_entry(chat: &str, entry: &serde_json::Value) -> Self {
        let message_id = entry["id"].as_str().map(str::to_owned);
        let delegation = if entry["delegation"].is_object() {
            message_id.as_ref().map(|id| DelegationRef {
                chat_id: chat.into(),
                entry_id: id.clone(),
                root_id: entry["replyTo"].as_str().unwrap_or(id).into(),
            })
        } else {
            entry["delegatedFrom"].as_object().and_then(|origin| {
                Some(DelegationRef {
                    chat_id: origin.get("chatId")?.as_str()?.into(),
                    entry_id: origin.get("entryId")?.as_str()?.into(),
                    root_id: origin.get("rootId")?.as_str()?.into(),
                })
            })
        };
        Self {
            message_id,
            delegation,
        }
    }
}

pub(crate) struct ChatJob {
    pub delivery: Option<Delivery>,
    pub context: TurnContext,
    pub seed: String,
    pub depth: u8,
}

pub(crate) struct Turn {
    pub delivery: Option<Delivery>,
    pub context: TurnContext,
    pub chat: String,
    pub task: String,
    pub(crate) depth: u8,
    pub(crate) spoken: u8,
    pub(crate) passed: bool,
    pub(crate) delegated: bool,
    pub(crate) denial_reason: Option<String>,
}

impl Turn {
    pub(crate) fn new(chat: &str, task: &str, depth: u8, context: TurnContext) -> Self {
        Self {
            delivery: None,
            context,
            chat: chat.into(),
            task: task.into(),
            depth,
            spoken: 0,
            passed: false,
            delegated: false,
            denial_reason: None,
        }
    }
}
