use crate::{domain::bot::BotDefaults, storage::Store};
use anyhow::Result;
use std::path::PathBuf;

pub(in crate::modules::bots) fn load_defaults(store: &Store) -> Result<BotDefaults> {
    store.tx(|repos, _| {
        Ok(BotDefaults::resolve(
            repos.settings.bot_preferences()?,
            std::env::var("HERDR_BOT_DEFAULT_CWD")
                .ok()
                .map(PathBuf::from),
            std::env::var("HERDR_BOT_DEFAULT_KIND").ok(),
            repos.bots.last_working_directory()?,
        ))
    })
}
