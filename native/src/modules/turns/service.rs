use crate::{
    domain::{need, string_field, strings, ChatJob, Turn},
    infrastructure::{herdr::has_error_code, AgentSessions},
    modules::transcript::{content, notice},
    storage::Store,
};
use anyhow::{bail, Result};
use serde_json::Value;
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
                    notice(
                        store,
                        target,
                        &format!("{bot} is reviewing a permission request."),
                    );
                    continue;
                }
                if sessions.active.lock().unwrap().contains_key(bot) {
                    notice(store, target, &format!("{bot} is busy in another chat."));
                    continue;
                }
                let live = match crate::modules::bots::connect_for_turn(store, &sessions.herdr, bot)
                {
                    Ok(live) => live,
                    Err(error) => {
                        notice(store, target, &format!("{bot}: {error:#}"));
                        continue;
                    }
                };
                if matches!(string_field(&live, "agent_status"), "working" | "blocked") {
                    // A reviewer can post its decision before its provider finishes the turn.
                    if !reviewers.contains(bot) {
                        notice(
                            store,
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
            let result = prompt(store, sessions, bot, &pane, target, &job.seed);
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
                notice(store, target, &format!("{bot}: {error:#}"));
            } else if !spoken && !turn.is_some_and(|active_turn| active_turn.passed) {
                if denied_bots.contains(bot) {
                    notice(
                        store,
                        target,
                        &format!("{bot} did not post a next step after the permission denial."),
                    );
                    continue;
                }
                notice(
                    store,
                    target,
                    &format!("{bot} finished without posting a reply."),
                );
            }
        }
        if !any_spoke {
            break;
        }
    }
    Ok(())
}
fn prompt(
    store: &Store,
    sessions: &AgentSessions,
    bot: &str,
    pane: &str,
    target: &str,
    seed: &str,
) -> Result<()> {
    let (profile, entries, room) = store.tx(|repos, _| {
        Ok((
            repos.chats.get(bot)?,
            repos.transcript.page(target, i64::MAX, 24)?,
            repos.chats.get(target)?,
        ))
    })?;
    let history = entries
        .iter()
        .filter(|entry| {
            matches!(
                string_field(entry, "kind"),
                "message" | "send-message" | "prompt"
            )
        })
        .map(|entry| {
            let author = string_field(&entry["author"], "name");
            if entry["kind"] == "prompt" {
                format!(
                    "{author} requested permission or input: {} [{}]",
                    content(entry),
                    string_field(entry, "status")
                )
            } else if let Some(decision) = entry["permissionDecision"].as_str() {
                format!("{author} ({decision}): {}", content(entry))
            } else {
                format!("{author}: {}", content(entry))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cli = sessions.herdr.cli.to_string_lossy().replace('\\', "/");
    let prompt=format!("You are {} (bot ID {bot}) in herdr-bot chat {target}. Role: {}. Members: {}.\n\
        Use the native CLI at {cli} from this pane. Run it with arguments: say {target} \"your reply\" (or pass {target} if finished). \
        Call the CLI to post; terminal output is private. In PowerShell invoke a quoted path with &. \
        To delegate, use message <bot-or-room-id> \"task\". Do not repeat earlier replies.\n\
        Current request:\n{}\nRecent conversation:\n{}",string_field(&profile,"name"),string_field(&profile,"description"),room["memberIds"],seed,history);
    run_or_resume_prompt(sessions, bot, pane, Some(&prompt))?;
    // Esc can finish the provider turn before the answer transaction commits.
    let denial = {
        let _roster = sessions.roster.lock().unwrap();
        sessions
            .active
            .lock()
            .unwrap()
            .get(bot)
            .filter(|turn| turn.spoken == 0 && !turn.passed)
            .and_then(|turn| turn.denial_reason.clone())
    };
    if let Some(reason) = denial {
        follow_up_on_denial(sessions, bot, pane, target, &reason)?;
    }
    Ok(())
}

fn follow_up_on_denial(
    sessions: &AgentSessions,
    bot: &str,
    pane: &str,
    target: &str,
    reason: &str,
) -> Result<()> {
    let herdr = &sessions.herdr;
    let live = herdr.run(&["agent", "get", pane])?;
    let agent = &live["agent"];
    if agent["name"] != bot
        || agent["pane_id"] != pane
        || !matches!(string_field(agent, "agent_status"), "idle" | "done")
    {
        bail!("bot_not_ready: the original pane is no longer available for the denial reply");
    }
    let cli = herdr.cli.to_string_lossy().replace('\\', "/");
    let instruction = format!("The permission request was denied. Reason: {reason}\n\
        Continue the conversation in its language. Respect the denial. Do not execute or retry the denied operation. \
        If a revised action is appropriate, request fresh permission for it. Otherwise explain what input you need. \
        Post your next step to the room with the native CLI at {cli} using arguments: say {target} \"your reply\". \
        In PowerShell use & before the quoted executable path.");
    run_or_resume_prompt(sessions, bot, pane, Some(&instruction))
}

fn wait_after_stalled_prompt(
    sessions: &AgentSessions,
    bot: &str,
    pane: &str,
    timeout_ms: u64,
    stalled: anyhow::Error,
) -> Result<Value> {
    let herdr = &sessions.herdr;
    // A stale "done" can belong to the previous turn. Wait for the new turn
    // to start without submitting the prompt a second time.
    let grace_ms = timeout_ms.min(30_000);
    match herdr.run_timeout(
        &[
            "agent",
            "wait",
            pane,
            "--until",
            "working",
            "--until",
            "blocked",
            "--timeout",
            &grace_ms.to_string(),
        ],
        grace_ms.div_ceil(1000) + 10,
    ) {
        Err(error) if has_error_code(&error, "timeout") => {
            let snapshot = herdr.run(&["agent", "get", pane])?;
            let agent = &snapshot["agent"];
            if agent["name"] == bot
                && agent["pane_id"] == pane
                && matches!(string_field(agent, "agent_status"), "done" | "idle")
                && !sessions
                    .active
                    .lock()
                    .unwrap()
                    .get(bot)
                    .is_some_and(|turn| turn.passed || turn.spoken > 0)
            {
                return Err(stalled);
            }
            Ok(snapshot)
        }
        result => result,
    }
}

pub(crate) fn run_or_resume_prompt(
    sessions: &AgentSessions,
    bot: &str,
    pane: &str,
    prompt: Option<&str>,
) -> Result<()> {
    let herdr = &sessions.herdr;
    let timeout = std::env::var("HERDR_BOT_TURN_TIMEOUT_MS")
        .ok()
        .and_then(|timeout| timeout.parse::<u64>().ok())
        .unwrap_or(180000)
        .clamp(1000, 3600000);
    let limit = timeout.to_string();
    let command_timeout = timeout.div_ceil(1000) + 10;
    let mut result = match prompt {
        Some(text) => match herdr.run_timeout(
            &["agent", "prompt", pane, text, "--wait", "--timeout", &limit],
            command_timeout,
        ) {
            Err(error) if has_error_code(&error, "agent_prompt_stalled") => Ok(
                wait_after_stalled_prompt(sessions, bot, pane, timeout, error)?,
            ),
            result => result,
        },
        None => herdr.run(&["agent", "get", pane]),
    };
    loop {
        // Once submitted, a slow turn and an approval both retain their original
        // chat and CLI reply channel. Only the status wait may be repeated.
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(error) if has_error_code(&error, "timeout") => {
                let snapshot = herdr.run(&["agent", "get", pane])?;
                if snapshot["agent"]["agent_status"] == "unknown" {
                    return Err(error);
                }
                snapshot
            }
            Err(error) => return Err(error),
        };
        let agent = &snapshot["agent"];
        if agent["name"] != bot || agent["pane_id"] != pane {
            bail!("bot_not_ready: pane has changed");
        }
        match string_field(agent, "agent_status") {
            "done" | "idle" => return Ok(()),
            "working" | "blocked" => {}
            status => bail!("bot_not_ready: agent status is {status}"),
        }
        result = herdr.run_timeout(
            &[
                "agent",
                "wait",
                pane,
                "--until",
                "done",
                "--until",
                "idle",
                "--timeout",
                &limit,
            ],
            command_timeout,
        );
    }
}
