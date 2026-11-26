use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde_json::{json, Value as V};
use std::{sync::LazyLock, thread, time::Duration};
static ROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(❯|›)?\s*(\d+)\.\s+(?:\[([ ✔✓xX])\]\s*)?(.+)$").unwrap());
static LETTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(❯|›)?\s*([A-Z])\s+()(.+)$").unwrap());
static QUESTION_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Question (\d+)/(\d+)\b").unwrap());

fn codex_menu(raw: &str) -> Option<V> {
    let lines: Vec<_> = raw
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect();
    let end = lines.len();
    let footer_start = lines.iter().rposition(|line| {
        let s = line.trim_start().to_ascii_lowercase();
        s.starts_with("press enter to confirm")
            || s.starts_with("enter continue")
            || s.contains("enter to submit answer")
            || s.contains("enter to submit all")
    })?;
    let footer = lines[footer_start..].join(" ").to_ascii_lowercase();
    if !footer.contains("esc ") || end - footer_start > 3 {
        return None;
    }
    let above = &lines[..footer_start];
    let is_update = footer.trim_start().starts_with("enter continue");
    let is_permission = footer.trim_start().starts_with("press enter to confirm");
    let heading = above.iter().rposition(|line| {
        let text = line.trim();
        if is_update {
            text.starts_with("Update available")
        } else if is_permission {
            text.starts_with("Would you like to ") || text.starts_with("Do you want to ")
        } else {
            QUESTION_HEADER.is_match(text)
        }
    })?;
    let form = &above[heading..];
    let header = form[0].trim();
    let first = form
        .iter()
        .position(|l| ROW.captures(l).is_some_and(|c| &c[2] == "1"));
    let input = form
        .iter()
        .position(|l| l.trim().starts_with("› Type your answer"));
    let start = first.or(input)?;
    let context: Vec<_> = form[1..start].iter().map(|l| l.trim()).collect();
    let (question, body) = if is_permission || is_update {
        (header.to_owned(), context)
    } else {
        (context.join(" "), vec![])
    };
    if question.is_empty() {
        return None;
    }
    let mut options: Vec<V> = vec![];
    let mut free = V::Null;
    let navigation = if first.is_some() {
        for line in &form[start..] {
            if let Some(row) = ROW.captures(line) {
                if row[2].parse::<usize>().ok()? != options.len() + 1 {
                    return None;
                }
                options.push(json!({"key":&row[2],"label":row[4].trim(),"detail":null,"checked":null,"selected":row.get(1).is_some()}));
            } else if line.starts_with("    ") {
                let last = options.last_mut()?;
                last["label"] = format!("{} {}", last["label"].as_str()?, line.trim()).into();
            } else {
                return None;
            }
        }
        if options.iter().filter(|o| o["selected"] == true).count() != 1 {
            return None;
        }
        "cursor"
    } else {
        if is_permission || is_update || start + 1 != form.len() {
            return None;
        }
        free = json!("text");
        options.push(json!({"key":"text","label":form[start].trim().trim_start_matches('›').trim(),"detail":null,"checked":null,"selected":false}));
        "text"
    };
    let labels: Vec<_> = options.iter().map(|o| (&o["key"], &o["label"])).collect();
    let signature = json!(["codex-menu", header, question, body, labels]).to_string();
    Some(
        json!({"kind":if is_permission {"permission"} else {"question"},"title":null,
        "header":if is_permission || is_update {None} else {Some(header)},
        "question":question,"body":body,"progress":null,"options":options,"multiSelect":false,
        "freeTextKey":free,"signature":signature,"navigation":navigation}),
    )
}
pub(super) fn parse_screen(raw: &str, provider: &str) -> V {
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
        if provider == "codex" {
            break;
        }
        let compact = lines
            .iter()
            .any(|l| l.contains("overrides auto mode for this command"));
        let rx = if compact && !lines.iter().any(|line| ROW.is_match(line)) {
            &*LETTER
        } else {
            &*ROW
        };
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

fn cursor_position(menu: &V) -> Option<usize> {
    menu["options"]
        .as_array()?
        .iter()
        .position(|option| option["selected"] == true)
}

pub(super) fn select_menu_option(
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

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &str = "Update available · 0.169.0 → 0.172.0\nRelease notes: https://github.com/openai/codex/releases/latest\n› 1. Update now (runs `npm install -g @openai/codex`)\n  2. Skip\n  3. Skip until next version\nenter continue · esc skip";

    #[test]
    fn codex_approval_keeps_command_context_and_wrapped_choices() {
        let screen = "previous chat?\n  Would you like to run the following command?\n\n  Reason: inspect the project\n  $ list files\n› 1. Yes, proceed (y)\n  2. Yes, and don't ask again for commands\n     that start with list (p)\n  3. No, and tell Codex what to do differently (esc)\n\n  Press enter to confirm or esc to cancel";
        let first = parse_screen(screen, "codex");
        assert_eq!(first["kind"], "permission");
        assert_eq!(
            first["question"],
            "Would you like to run the following command?"
        );
        assert_eq!(
            first["body"],
            json!(["Reason: inspect the project", "$ list files"])
        );
        assert_eq!(
            first["options"][1]["label"],
            "Yes, and don't ask again for commands that start with list (p)"
        );
        let second = parse_screen(
            &screen.replace("› 1.", "  1.").replace("  2.", "› 2."),
            "codex",
        );
        let mut keys = Vec::new();
        select_menu_option(
            &first,
            "2",
            || Ok(second.clone()),
            |key| {
                keys.push(key.to_owned());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(keys, ["down", "enter"]);
        let different = parse_screen(&screen.replace("$ list files", "$ list users"), "codex");
        assert_ne!(different["signature"], first["signature"]);
    }

    #[test]
    fn codex_questions_accept_numbered_choices_and_direct_text() {
        let screen = "  Question 1/2 (2 unanswered)\n  Choose a color.\n  › 1. Red\n    2. Blue\n  tab to add notes | enter to submit answer\n  ←/→ to navigate questions | esc to interrupt";
        let choices = parse_screen(screen, "codex");
        assert_eq!(choices["question"], "Choose a color.");
        assert_eq!(choices["options"][1]["label"], "Blue");
        assert_eq!(choices["navigation"], "cursor");
        let text = parse_screen("Question 2/2 (1 unanswered)\nShare details.\n› Type your answer (optional)\nenter to submit all | esc to interrupt", "codex");
        assert_eq!(text["navigation"], "text");
        assert_eq!(text["freeTextKey"], "text");
        assert_eq!(
            parse_screen(&screen.replace("› 1.", "  1."), "codex")["kind"],
            "unknown"
        );
    }

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
