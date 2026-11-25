use crate::{store::*, Engine};
use anyhow::{anyhow, bail, Result};
use regex::Regex;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value as V};
use std::{sync::LazyLock, thread, time::Duration};

static ROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(❯|›)?\s*(\d+)\.\s+(?:\[([ ✔✓xX])\]\s*)?(.+)$").unwrap());
static LETTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(❯|›)?\s*([A-Z])\s+()(.+)$").unwrap());
fn codex_menu(raw: &str) -> Option<V> {
    let lines: Vec<_> = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let footer = *lines.last()?;
    if !footer.starts_with("enter continue") || !footer.contains("esc ") {
        return None;
    }
    let rows_end = lines.len() - 1;
    let rows_start = lines[..rows_end]
        .iter()
        .rposition(|l| !ROW.is_match(l))
        .map_or(0, |i| i + 1);
    let heading = lines[..rows_start]
        .iter()
        .rposition(|l| l.starts_with("Update available"))?;
    let before = &lines[heading..rows_start];
    let mut options = vec![];
    for (index, line) in lines[rows_start..rows_end].iter().enumerate() {
        let row = ROW.captures(line)?;
        if row[2].parse::<usize>().ok()? != index + 1 {
            return None;
        }
        options.push(json!({"key":&row[2],"label":row[4].trim(),"detail":null,"checked":null,"selected":row.get(1).is_some()}));
    }
    if options.len() < 2 || options.iter().filter(|o| o["selected"] == true).count() != 1 {
        return None;
    }
    let body = &before[1..];
    let labels: Vec<_> = options.iter().map(|o| (&o["key"], &o["label"])).collect();
    let signature = json!(["codex-menu", before[0], body, labels]).to_string();
    Some(
        json!({"kind":"question","title":null,"header":null,"question":before[0],"body":body,
        "progress":null,"options":options,"multiSelect":false,"freeTextKey":null,"signature":signature,"navigation":"cursor"}),
    )
}

fn parse_screen(raw: &str, provider: &str) -> V {
    if provider == "codex" {
        if let Some(menu) = codex_menu(raw) {
            return menu;
        }
    }
    let sections: Vec<Vec<&str>> = raw
        .lines()
        .collect::<Vec<_>>()
        .split(|s| s.trim().starts_with("────────"))
        .map(|s| s.to_vec())
        .collect();
    for lines in sections.iter().rev() {
        let compact = lines
            .iter()
            .any(|l| l.contains("overrides auto mode for this command"));
        let rx = if compact { &*LETTER } else { &*ROW };
        let Some(first) = lines.iter().position(|l| rx.is_match(l)) else {
            continue;
        };
        let before: Vec<_> = lines[..first]
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let question = before.last().copied().unwrap_or("Select an option");
        if !(compact || question.contains(['?', '？']) || question.contains("Review your answers"))
        {
            continue;
        }
        let mut options = vec![];
        let mut free = V::Null;
        let mut multi = false;
        for l in &lines[first..] {
            if let Some(cap) = rx.captures(l) {
                let label = cap[4].trim();
                if label.eq_ignore_ascii_case("chat about this") {
                    continue;
                }
                let key = cap[2].to_lowercase();
                if label
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case("type something")
                {
                    free = json!(key);
                }
                let checked = cap
                    .get(3)
                    .filter(|x| !x.as_str().is_empty())
                    .map(|x| x.as_str() != " ");
                multi |= checked.is_some();
                options.push(json!({"key":key,"label":label,"detail":null,"selected":cap.get(1).is_some(),"checked":checked}));
            }
        }
        let sig = json!([
            before,
            options
                .iter()
                .map(|o| (&o["key"], &o["label"]))
                .collect::<Vec<_>>()
        ])
        .to_string();
        return json!({"kind":if compact||question.contains("proceed"){"permission"}else{"question"},"title":null,"header":null,
            "progress":null,"question":question,"body":before[..before.len().saturating_sub(1)],"options":options,"multiSelect":multi,"freeTextKey":free,"signature":sig});
    }
    let body = raw
        .lines()
        .rev()
        .filter(|s| !s.trim().is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let question = body
        .iter()
        .find(|line| line.contains(['?', '？']))
        .or_else(|| body.first())
        .copied()
        .unwrap_or("Open the agent terminal to continue.");
    let detail: Vec<_> = body
        .iter()
        .copied()
        .filter(|line| *line != question)
        .collect();
    json!({"kind":"unknown","title":null,"header":null,"question":question,"body":detail,"progress":null,"options":[],"multiSelect":false,"freeTextKey":null,"signature":body.join("\n")})
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &str = "Update available · 0.169.0 → 0.172.0\nRelease notes: https://github.com/openai/codex/releases/latest\n› 1. Update now (runs `npm install -g @openai/codex`)\n  2. Skip\n  3. Skip until next version\nenter continue · esc skip";

    #[test]
    fn update_choices_follow_the_visible_cursor_before_confirming() {
        let first = parse_screen(MENU, "codex");
        assert_eq!(first["options"].as_array().unwrap().len(), 3);
        assert_eq!(first["body"].as_array().unwrap().len(), 1);
        let second = parse_screen(
            &MENU.replace("› 1.", "  1.").replace("  2.", "› 2."),
            "codex",
        );
        let third = parse_screen(
            &MENU.replace("› 1.", "  1.").replace("  3.", "› 3."),
            "codex",
        );
        let mut screens = [second, third].into_iter();
        let mut keys = vec![];
        select_menu_option(
            &first,
            "3",
            || Ok(screens.next().unwrap()),
            |k| {
                keys.push(k.to_owned());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(keys, ["down", "down", "enter"]);
    }

    #[test]
    fn changed_update_menu_never_receives_enter() {
        let first = parse_screen(MENU, "codex");
        let changed = parse_screen(&MENU.replace("0.172.0", "0.173.0"), "codex");
        let mut keys = vec![];
        assert!(select_menu_option(
            &first,
            "2",
            || Ok(changed.clone()),
            |k| {
                keys.push(k.to_owned());
                Ok(())
            }
        )
        .is_err());
        assert_eq!(keys, ["down"]);
    }
}
fn cursor_position(menu: &V) -> Option<usize> {
    menu["options"]
        .as_array()?
        .iter()
        .position(|option| option["selected"] == true)
}

fn select_menu_option(
    initial: &V,
    key: &str,
    mut read: impl FnMut() -> Result<V>,
    mut press: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let target = initial["options"]
        .as_array()
        .and_then(|rows| rows.iter().position(|o| o["key"] == key))
        .ok_or_else(|| anyhow!("invalid_answer: unknown menu option"))?;
    let mut position =
        cursor_position(initial).ok_or_else(|| anyhow!("prompt_changed: no menu cursor"))?;
    while position != target {
        let (direction, next) = if target > position {
            ("down", position + 1)
        } else {
            ("up", position - 1)
        };
        press(direction)?;
        let mut moved = false;
        for _ in 0..10 {
            thread::sleep(Duration::from_millis(60));
            let current = read()?;
            if current["signature"] != initial["signature"] {
                bail!("prompt_changed: menu changed");
            }
            if cursor_position(&current) == Some(next) {
                moved = true;
                break;
            }
            if cursor_position(&current) != Some(position) {
                bail!("prompt_changed: menu selection changed");
            }
        }
        if !moved {
            bail!("herdr_error: menu cursor did not move");
        }
        position = next;
    }
    press("enter")
}

impl Engine {
    fn read_prompt(&self, pane: &str, provider: &str) -> Result<V> {
        Ok(parse_screen(
            &self.herdr.text(
                &[
                    "agent",
                    "read",
                    pane,
                    "--source",
                    "detection",
                    "--lines",
                    "80",
                ],
                8,
            )?,
            provider,
        ))
    }
    pub(crate) fn refresh(&self) -> Result<()> {
        let Ok(_guard) = self.roster.try_lock() else {
            return Ok(());
        };
        let chats = self.tx(|c, _| list(c))?;
        if !chats.iter().any(|v| v["kind"] == "bot") {
            return Ok(());
        }
        let agents = self.herdr.agents()?;
        for v in chats.iter().filter(|v| v["kind"] == "bot") {
            let id = need(v, "id")?;
            let live = agents
                .iter()
                .find(|a| a["name"] == id && a["pane_id"] == v["bot"]["paneId"]);
            let status = live
                .and_then(|a| a["agent_status"].as_str())
                .unwrap_or("offline");
            let prompt = if status == "blocked" {
                self.read_prompt(need(&v["bot"], "paneId")?, s(&v["bot"], "provider"))
                    .unwrap_or(V::Null)
            } else {
                V::Null
            };
            let target = self
                .active
                .lock()
                .unwrap()
                .get(id)
                .map(|t| t.chat.clone())
                .unwrap_or_else(|| id.into());
            self.tx(|c,ev|{let mut row=chat(c,id)?;if row["bot"]["status"]==status && row["bot"]["pendingPrompt"]==prompt{return Ok(())}
                let pending:Option<(String,V)>=c.query_row("SELECT chat,data FROM entries WHERE json_extract(data,'$.kind')='prompt' AND json_extract(data,'$.author.id')=? AND json_extract(data,'$.status')='pending' ORDER BY rowid DESC LIMIT 1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
                let mut same=false;
                if let Some((prev_chat,mut e))=pending{same=!prompt.is_null() && e["prompt"]["signature"]==prompt["signature"];
                    if same{e["prompt"]=prompt.clone()}else{e["status"]="resolved".into()}update_entry(c,&prev_chat,&e,ev)?;}
                if !same && !prompt.is_null(){append(c,&target,json!({"kind":"prompt","prompt":prompt,"status":"pending","author":{"id":id,"name":row["name"]}}),ev)?;row=chat(c,id)?;}
                row["bot"]["status"]=status.into();row["bot"]["pendingPrompt"]=prompt;row["isRunning"]=(status=="working").into();
                row["currentActivity"]=if status=="working"{json!({"verb":"working"})}else{V::Null};
                row["awaitingUserResponse"]=match status{"blocked"=>json!({"reason":"approval"}),"offline"=>json!({"reason":"offline"}),_=>V::Null};change(c,&row,ev)
            })?;
        }
        Ok(())
    }
    pub(crate) fn answer(&self, p: &V) -> Result<V> {
        let _guard = self.roster.lock().unwrap();
        let id = need(p, "botId")?;
        let row = self.tx(|c, _| chat(c, id))?;
        let pane = need(&row["bot"], "paneId")?;
        let live = self.herdr.run(&["agent", "get", pane])?;
        if live["agent"]["agent_status"] != "blocked" || live["agent"]["name"] != id {
            bail!("not_blocked: agent is no longer waiting")
        }
        let provider = s(&live["agent"], "agent");
        let prompt = self.read_prompt(pane, provider)?;
        if prompt["signature"] != p["signature"] {
            bail!("prompt_changed: form changed")
        }
        let press = |key: &str| self.herdr.run(&["agent", "send-keys", pane, key]);
        let answer = match need(p, "action")? {
            "option" => {
                let key = need(p, "key")?;
                let option = prompt["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|v| v["key"] == key)
                    .ok_or_else(|| anyhow!("invalid_answer: unknown option"))?;
                if prompt["navigation"] == "cursor" {
                    select_menu_option(
                        &prompt,
                        key,
                        || self.read_prompt(pane, provider),
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
                V::Null
            }
            "text" if prompt["freeTextKey"].is_string() => {
                let text = need(p, "text")?.lines().collect::<Vec<_>>().join(" ");
                press(s(&prompt, "freeTextKey"))?;
                thread::sleep(Duration::from_millis(500));
                self.herdr.run(&["pane", "send-text", pane, &text])?;
                thread::sleep(Duration::from_millis(500));
                press("enter")?;
                json!({"kind":"text","text":text})
            }
            _ => bail!("invalid_answer: unsupported action"),
        };
        self.tx(|c,ev|{if !answer.is_null() && prompt["multiSelect"]!=true{
            let pending:Option<(String,V)>=c.query_row("SELECT chat,data FROM entries WHERE json_extract(data,'$.author.id')=? AND json_extract(data,'$.status')='pending' AND json_extract(data,'$.prompt.signature')=? ORDER BY rowid DESC LIMIT 1",params![id,need(p,"signature")?],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            if let Some((target,mut entry))=pending{entry["answer"]=answer;entry["status"]="answered".into();update_entry(c,&target,&entry,ev)?;}
        }Ok(json!({"chat":chat(c,id)?}))})
    }
}
