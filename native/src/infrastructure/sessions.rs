use super::herdr::Herdr;
use crate::domain::Turn;
use std::{collections::HashMap, sync::Mutex};

pub(crate) struct AgentSessions {
    pub herdr: Herdr,
    pub roster: Mutex<()>,
    pub active: Mutex<HashMap<String, Turn>>,
}

impl AgentSessions {
    pub(crate) fn new(herdr: Herdr) -> Self {
        Self {
            herdr,
            roster: Mutex::default(),
            active: Mutex::default(),
        }
    }
}
