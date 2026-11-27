#[derive(Clone)]
pub(crate) struct Delivery {
    pub chat: String,
    pub bot: String,
    pub seq: i64,
}

pub(crate) struct ChatJob {
    pub delivery: Option<Delivery>,
    pub seed: String,
    pub depth: u8,
}

pub(crate) struct Turn {
    pub delivery: Option<Delivery>,
    pub chat: String,
    pub task: String,
    pub(crate) depth: u8,
    pub(crate) spoken: u8,
    pub(crate) passed: bool,
    pub(crate) delegated: bool,
    pub(crate) denial_reason: Option<String>,
}

impl Turn {
    pub(crate) fn new(chat: &str, task: &str, depth: u8) -> Self {
        Self {
            delivery: None,
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
