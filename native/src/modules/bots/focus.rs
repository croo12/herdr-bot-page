use crate::{domain::bot::BotId, infrastructure::herdr::Herdr, storage::Store};
use anyhow::Result;

pub(crate) fn focus_bot(store: &Store, herdr: &Herdr, bot_id: &BotId) -> Result<()> {
    let registered = store.tx(|repos, _| repos.bots.registration(bot_id))?;
    herdr.focus(registered.session.required_pane()?)
}
