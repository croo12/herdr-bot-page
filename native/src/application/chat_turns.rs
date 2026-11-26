use crate::{
    domain::{need, string_field, strings, ChatJob, Turn},
    infrastructure::AgentSessions,
    modules::{bots, turns},
    storage::Store,
};
use anyhow::Result;
use std::collections::HashSet;
pub(crate) fn run_chat(
    store: &Store,
    sessions: &AgentSessions,
    target: &str,
    job: ChatJob,
) -> Result<()> {
    let chat = store.tx(|repos, _| repos.chats.get(target))?;
    let room = chat["kind"] == "room";
    let mut members = if room {
        strings(&chat["memberIds"])
    } else {
        vec![target.to_owned()]
    };
    members.retain(|member_id| job.exclude.as_ref() != Some(member_id));
    let seed = job.seed.to_lowercase();
    members.sort_by_key(|member_id| seed.find(&format!("@{member_id}")).unwrap_or(usize::MAX));
    let mut turns = 0;
    let mut denied_bots = HashSet::new();
    let mut reviewers = HashSet::new();
    for _ in 0..if room { 3 } else { 1 } {
        let mut any_spoke = false;
        for bot in &members {
            if denied_bots.contains(bot) {
                continue;
            }
            if turns >= 10 {
                return Ok(());
            }
            turns += 1;
            let pane = {
                let _roster = sessions.roster.lock().unwrap();
                if store.tx(|repos, _| repos.chats.get(bot)).is_err() {
                    continue;
                }
                let target_row = store.tx(|repos, _| repos.chats.get(target))?;
                if room && !strings(&target_row["memberIds"]).contains(bot) {
                    continue;
                }
                if store.tx(|repos, _| repos.reviews.pending_for(bot))? {
                    store.notice(target, &format!("{bot} is reviewing a permission request."));
                    continue;
                }
                if sessions.active.lock().unwrap().contains_key(bot) {
                    store.notice(target, &format!("{bot} is busy in another chat."));
                    continue;
                }
                let live = match bots::connect_for_turn(store, &sessions.herdr, bot) {
                    Ok(live) => live,
                    Err(error) => {
                        store.notice(target, &format!("{bot}: {error:#}"));
                        continue;
                    }
                };
                if matches!(string_field(&live, "agent_status"), "working" | "blocked") {
                    // A reviewer can post its decision before its provider finishes the turn.
                    if !reviewers.contains(bot) {
                        store.notice(
                            target,
                            &format!("{bot}: bot_not_ready: {}", live["agent_status"]),
                        );
                    }
                    continue;
                }
                let pane = need(&live, "pane_id")?.to_owned();
                sessions
                    .active
                    .lock()
                    .unwrap()
                    .insert(bot.clone(), Turn::new(target, &job.seed, job.depth));
                pane
            };
            let result = turns::run_chat_turn(store, sessions, bot, &pane, target, &job.seed);
            let turn = {
                let _roster = sessions.roster.lock().unwrap();
                sessions.active.lock().unwrap().remove(bot)
            };
            if let Some(turn) = &turn {
                if turn.denial_reason.is_some() {
                    denied_bots.insert(bot.clone());
                }
                reviewers.extend(turn.reviewers.iter().cloned());
            }
            let spoken = turn
                .as_ref()
                .is_some_and(|active_turn| active_turn.spoken > 0);
            any_spoke |= spoken;
            if let Err(error) = result {
                store.notice(target, &format!("{bot}: {error:#}"));
            } else if !spoken && !turn.is_some_and(|active_turn| active_turn.passed) {
                if denied_bots.contains(bot) {
                    store.notice(
                        target,
                        &format!("{bot} did not post a next step after the permission denial."),
                    );
                    continue;
                }
                store.notice(target, &format!("{bot} finished without posting a reply."));
            }
        }
        if !any_spoke {
            break;
        }
    }
    Ok(())
}
