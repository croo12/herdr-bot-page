use super::lib::{register_bot, BotName, CreateBotInput, Registration};
use crate::{
    domain::{need, ChatJob},
    infrastructure::AgentSessions,
    storage::Store,
};
use anyhow::Result;
use serde_json::{json, Value};

pub(crate) fn quick_create_bot(
    store: &Store,
    sessions: &AgentSessions,
    mut input: CreateBotInput,
    locale: &str,
    enqueue_greeting: &dyn Fn(&str, &str, ChatJob),
) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    if matches!(&input.profile.name, BotName::Explicit(name) if name.trim().is_empty()) {
        input.profile.name = BotName::Provider;
    }
    let profile = match register_bot(store, &sessions.herdr, input)? {
        Registration::Created(bot_id, profile) => {
            start_conversation(store, &bot_id, locale, enqueue_greeting)?;
            profile
        }
        Registration::AlreadyCreated(profile) => profile,
    };
    Ok(json!({"chat": profile}))
}

fn start_conversation(
    store: &Store,
    bot_id: &crate::domain::bot::BotId,
    locale: &str,
    enqueue_greeting: &dyn Fn(&str, &str, ChatJob),
) -> Result<()> {
    let bot_id = bot_id.as_str();
    let dm = store.tx(|repos, events| repos.chats.open_direct(bot_id, None, events))?;
    enqueue_greeting(
        bot_id,
        need(&dm, "id")?,
        ChatJob {
            delivery: None,
            context: Default::default(),
            seed: format!("Introduce yourself in {locale} in one sentence."),
            depth: 0,
        },
    );
    Ok(())
}
