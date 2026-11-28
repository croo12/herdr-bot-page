use super::lib::{register_bot, CreateBotInput, Registration};
use crate::{domain::bot::BotId, infrastructure::herdr::Herdr, storage::Store};
use anyhow::Result;

pub(crate) fn create_bot(store: &Store, herdr: &Herdr, input: CreateBotInput) -> Result<BotId> {
    match register_bot(store, herdr, input)? {
        Registration::Created(bot_id) | Registration::AlreadyCreated(bot_id) => Ok(bot_id),
    }
}
