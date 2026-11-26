use std::collections::HashSet;

pub(crate) struct ChatJob {
    pub seed: String,
    pub depth: u8,
    pub exclude: Option<String>,
}

pub(crate) struct Turn {
    pub chat: String,
    pub task: String,
    pub(crate) depth: u8,
    pub(crate) spoken: u8,
    pub(crate) passed: bool,
    pub(crate) delegated: bool,
    pub(crate) denial_reason: Option<String>,
    pub(crate) reviewers: HashSet<String>,
}

impl Turn {
    pub(crate) fn new(chat: &str, task: &str, depth: u8) -> Self {
        Self {
            chat: chat.into(),
            task: task.into(),
            depth,
            spoken: 0,
            passed: false,
            delegated: false,
            denial_reason: None,
            reviewers: HashSet::new(),
        }
    }

    pub(crate) fn record_permission_answer(
        &mut self,
        denial: Option<&str>,
        reviewer: Option<&str>,
    ) {
        if let Some(reason) = denial {
            self.denial_reason = Some(reason.to_owned());
        }
        if let Some(reviewer) = reviewer {
            self.reviewers.insert(reviewer.to_owned());
        }
    }
}
