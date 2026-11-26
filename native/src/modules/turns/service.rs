use crate::{
    domain::{content, string_field},
    infrastructure::{herdr::has_error_code, AgentSessions},
    storage::Store,
};
use anyhow::{bail, Result};
use serde_json::Value;

pub(crate) fn run_chat_turn(
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
