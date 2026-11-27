use crate::{
    domain::{need, string_field, strings, ChatJob, Turn},
    infrastructure::AgentSessions,
    modules::{bots, turns},
    storage::Store,
};
use anyhow::{anyhow, Result};

pub(crate) fn run_chat(
    store: &Store,
    sessions: &AgentSessions,
    target: &str,
    job: &ChatJob,
) -> Result<()> {
    let (bot, pane) = {
        let _roster = sessions.roster.lock().unwrap();
        let chat = store.tx(|repos, _| repos.chats.get(target))?;
        let members = strings(&chat["memberIds"]);
        let bot = match &job.delivery {
            Some(delivery) if members.contains(&delivery.bot) => delivery.bot.clone(),
            Some(_) => return Ok(()),
            // The only unsolicited turn is the introduction in a newly created user DM.
            None => members
                .first()
                .ok_or_else(|| anyhow!("unknown_bot: empty direct chat"))?
                .clone(),
        };
        if chat["archived"] == true
            || sessions.active.lock().unwrap().contains_key(&bot)
            || store.tx(|repos, _| repos.reviews.pending_for(&bot))?
        {
            return Ok(());
        }
        let live = bots::connect_for_turn(store, &sessions.herdr, &bot)?;
        if matches!(string_field(&live, "agent_status"), "working" | "blocked") {
            return Ok(());
        }
        let pane = need(&live, "pane_id")?.to_owned();
        if let Some(delivery) = &job.delivery {
            if !store.tx(|repos, _| repos.inbox.start(delivery))? {
                return Ok(());
            }
        }
        if let Some(link) = &job.context.delegation {
            store.tx(|repos, events| {
                repos.set_delegation_state(link, "running", None, None, events)
            })?;
        }
        let mut turn = Turn::new(target, &job.seed, job.depth, job.context.clone());
        turn.delivery = job.delivery.clone();
        sessions.active.lock().unwrap().insert(bot.clone(), turn);
        (bot, pane)
    };
    let result = turns::run_chat_turn(
        store,
        sessions,
        &bot,
        &pane,
        target,
        &job.seed,
        job.context.message_id.as_deref(),
    );
    let _roster = sessions.roster.lock().unwrap();
    let turn = sessions.active.lock().unwrap().remove(&bot);
    result?;
    if let Some(turn) = turn {
        if !turn.passed && turn.spoken == 0 {
            if job.context.delegation.is_some() {
                anyhow::bail!(
                    "{bot} finished without posting a reply or accepting the delegation."
                );
            }
            store.notice(target, &format!("{bot} finished without posting a reply."));
        }
    }
    Ok(())
}
