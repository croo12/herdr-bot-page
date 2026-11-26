use super::screen::{parse_screen, select_menu_option};
use crate::modules::reviews;
use crate::{
    domain::{need, string_field},
    modules::{
        chats::change,
        transcript::{append, update_entry},
    },
    Engine,
};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::{thread, time::Duration};
fn read_prompt(engine: &Engine, pane: &str, provider: &str) -> Result<Value> {
    Ok(parse_screen(
        &engine.herdr.text(
            &[
                "agent",
                "read",
                pane,
                "--source",
                "detection",
                "--lines",
                "240",
            ],
            8,
        )?,
        provider,
    ))
}
pub(crate) fn refresh(engine: &Engine) -> Result<()> {
    let Ok(_guard) = engine.roster.try_lock() else {
        return Ok(());
    };
    let chats = engine.tx(|repos, _| repos.chats.list())?;
    if !chats.iter().any(|chat| chat["kind"] == "bot") {
        return Ok(());
    }
    let agents = engine.herdr.agents()?;
    for chat in chats.iter().filter(|chat| chat["kind"] == "bot") {
        let id = need(chat, "id")?;
        let live = agents
            .iter()
            .find(|agent| agent["name"] == id && agent["pane_id"] == chat["bot"]["paneId"]);
        let status = live
            .and_then(|agent| agent["agent_status"].as_str())
            .unwrap_or("offline");
        let prompt = if status == "blocked" {
            read_prompt(
                engine,
                need(&chat["bot"], "paneId")?,
                string_field(&chat["bot"], "provider"),
            )
            .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let active_chat = engine
            .active
            .lock()
            .unwrap()
            .get(id)
            .map(|turn| turn.chat.clone());
        engine.tx(|repos, events| {
            let mut row = repos.chats.get(id)?;
            if row["bot"]["status"] == status && row["bot"]["pendingPrompt"] == prompt {
                return Ok(());
            }
            let pending = repos.prompts.pending(id)?;
            // Herdr can display an already answered menu for one more refresh.
            if pending.is_none() && !prompt.is_null() && row["bot"]["pendingPrompt"]["signature"] == prompt["signature"] {
                return Ok(());
            }
            let target = active_chat.as_deref().or_else(|| pending.as_ref().map(|(chat, _)| chat.as_str())).unwrap_or(id).to_owned();
            let mut same_prompt = false;
            if let Some((previous_chat, mut entry)) = pending {
                same_prompt = !prompt.is_null() && (entry["prompt"]["signature"] == prompt["signature"] || entry["prompt"]["kind"] == "unknown");
                if same_prompt { entry["prompt"] = prompt.clone(); } else { entry["status"] = "resolved".into(); }
                update_entry(repos, &previous_chat, &entry, events)?;
            }
            if !same_prompt && !prompt.is_null() {
                let entry = json!({"kind": "prompt", "prompt": prompt, "status": "pending", "author": {"id": id, "name": row["name"]}});
                append(repos, &target, entry, events)?;
                row = repos.chats.get(id)?;
            }
            row["bot"]["status"] = status.into();
            row["bot"]["pendingPrompt"] = prompt;
            row["isRunning"] = (status == "working").into();
            row["currentActivity"] = if status == "working" { json!({"verb":"working"}) } else { Value::Null };
            row["awaitingUserResponse"] = match status {
                "blocked" => json!({"reason": "approval"}),
                "offline" => json!({"reason": "offline"}),
                _ => Value::Null,
            };
            change(repos, &row, events)
        })?;
    }
    if chats.iter().any(|chat| chat["kind"] == "room") {
        reviews::dispatch_reviews(engine, &agents)?;
    }
    Ok(())
}
pub(crate) fn answer(engine: &Engine, params: &Value) -> Result<Value> {
    let _guard = engine.roster.lock().unwrap();
    answer_locked(engine, params, None)
}
pub(crate) fn answer_locked(
    engine: &Engine,
    params: &Value,
    review: Option<&Value>,
) -> Result<Value> {
    let id = need(params, "botId")?;
    let row = engine.tx(|repos, _| repos.chats.get(id))?;
    let pane = need(&row["bot"], "paneId")?;
    let live = engine.herdr.run(&["agent", "get", pane])?;
    if live["agent"]["agent_status"] != "blocked" || live["agent"]["name"] != id {
        bail!("not_blocked: agent is no longer waiting")
    }
    let provider = need(&row["bot"], "provider")?;
    let prompt = read_prompt(engine, pane, provider)?;
    if prompt["signature"] != params["signature"] {
        bail!("prompt_changed: form changed")
    }
    let pending = engine.tx(|repos, _| {
        Ok(repos
            .prompts
            .matching(id, need(params, "signature")?)?
            .is_some())
    })?;
    if !pending {
        bail!("prompt_changed: request already answered")
    }
    let press = |key: &str| engine.herdr.run(&["agent", "send-keys", pane, key]);
    let answer = match need(params, "action")? {
        "option" => {
            let key = need(params, "key")?;
            let option = prompt["options"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|option| option["key"] == key)
                .ok_or_else(|| anyhow!("invalid_answer: unknown option"))?;
            if prompt["navigation"] == "cursor" {
                select_menu_option(
                    &prompt,
                    key,
                    || read_prompt(engine, pane, provider),
                    |key| press(key).map(|_| ()),
                )?;
            } else {
                press(key)?;
            }
            json!({"kind":"option","key":key,"label":option["label"]})
        }
        "cancel" => {
            press("esc")?;
            json!({"kind":"cancelled"})
        }
        "next" if prompt["multiSelect"] == true => {
            press("tab")?;
            Value::Null
        }
        "text" if prompt["freeTextKey"].is_string() => {
            let text = need(params, "text")?.lines().collect::<Vec<_>>().join(" ");
            if prompt["navigation"] != "text" {
                press(string_field(&prompt, "freeTextKey"))?;
                thread::sleep(Duration::from_millis(500));
            }
            engine.herdr.run(&["pane", "send-text", pane, &text])?;
            thread::sleep(Duration::from_millis(500));
            press("enter")?;
            json!({"kind":"text","text":text})
        }
        _ => bail!("invalid_answer: unsupported action"),
    };
    engine.tx(|repos, events| {
        if !answer.is_null() && prompt["multiSelect"] != true {
            if let Some((target, mut entry)) = repos.prompts.matching(id, need(params, "signature")?)? {
                if let Some(review) = review {
                    entry["permissionReview"] = review.clone();
                } else if entry["permissionReview"]["decision"] == "pending" {
                    entry.as_object_mut().unwrap().remove("permissionReview");
                }
                entry["answer"] = answer;
                entry["status"] = "answered".into();
                update_entry(repos, &target, &entry, events)?;
                if let Some(review) = review {
                    reviews::append_reply(repos, events, &target, &entry, review)?;
                } else {
                    let text = match entry["answer"]["kind"].as_str() {
                        Some("option") => string_field(&entry["answer"], "label"),
                        Some("text") => string_field(&entry["answer"], "text"),
                        _ => "Esc",
                    };
                    let reply = json!({
                        "kind": "message", "role": "user", "author": {"id": "user", "name": engine.user},
                        "content": text, "replyTo": entry["id"], "isStreaming": false
                    });
                    append(repos, &target, reply, events)?;
                }
            }
        }
        Ok(json!({"chat": repos.chats.get(id)?}))
    })
}
