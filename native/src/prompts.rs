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
fn parse_screen(raw: &str) -> V {
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
    json!({"kind":"unknown","title":null,"header":null,"question":question,"body":body,"progress":null,"options":[],"multiSelect":false,"freeTextKey":null,"signature":body.join("\n")})
}
impl Engine {
    fn read_prompt(&self, pane: &str) -> Result<V> {
        Ok(parse_screen(&self.herdr.text(
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
        )?))
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
                self.read_prompt(need(&v["bot"], "paneId")?)
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
        let prompt = self.read_prompt(pane)?;
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
                press(key)?;
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
