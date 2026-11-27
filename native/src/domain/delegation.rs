#[derive(Clone)]
pub(crate) struct DelegationRef {
    pub chat_id: String,
    pub entry_id: String,
    pub root_id: String,
}

pub(crate) enum DelegationUpdate {
    Queued,
    Running { last_error: Option<String> },
    Waiting { chat_id: String, prompt_id: String },
    Completed,
    Failed(String),
}
