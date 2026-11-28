use super::lib::{register_bot, BotName, CreateBotInput, Registration};
use crate::{
    domain::{bot::BotId, ChatJob},
    infrastructure::herdr::Herdr,
    storage::Store,
};
use anyhow::Result;

pub(crate) fn quick_create_bot(
    store: &Store,
    herdr: &Herdr,
    mut input: CreateBotInput,
    locale: &str,
    enqueue_greeting: &dyn Fn(&str, &str, ChatJob),
) -> Result<BotId> {
    if matches!(&input.profile.name, BotName::Explicit(name) if name.trim().is_empty()) {
        input.profile.name = BotName::Provider;
    }
    match register_bot(store, herdr, input)? {
        Registration::Created(bot_id) => {
            start_conversation(store, &bot_id, locale, enqueue_greeting)?;
            Ok(bot_id)
        }
        Registration::AlreadyCreated(bot_id) => Ok(bot_id),
    }
}

fn start_conversation(
    store: &Store,
    bot_id: &BotId,
    locale: &str,
    enqueue_greeting: &dyn Fn(&str, &str, ChatJob),
) -> Result<()> {
    let chat_id = store.tx(|repos, events| repos.chats.open_user_direct_id(bot_id, events))?;
    enqueue_greeting(
        bot_id.as_str(),
        &chat_id,
        ChatJob {
            delivery: None,
            context: Default::default(),
            seed: format!("Introduce yourself in {locale} in one sentence."),
            depth: 0,
        },
    );
    Ok(())
}
