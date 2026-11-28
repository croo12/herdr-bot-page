use super::{capture, command};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
fn codex_catalog() -> Result<Value> {
    let mut child = provider_command("codex")
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let result = (|| -> Result<Value> {
            let init = json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"herdr-bot","version":"1.3.0"}}});
            writeln!(input, "{init}")?;
            for line in BufReader::new(output).lines() {
                let Ok(frame) = serde_json::from_str::<Value>(&line?) else {
                    continue;
                };
                if !frame["error"].is_null() {
                    bail!("Codex: {}", frame["error"]);
                }
                match frame["id"].as_i64() {
                    Some(1) => writeln!(
                        input,
                        "{}",
                        json!({"id":2,"method":"model/list","params":{}})
                    )?,
                    Some(2) => return Ok(frame["result"].clone()),
                    _ => {}
                }
            }
            bail!("Codex exited before returning its model list")
        })();
        let _ = tx.send(result);
    });
    let result = rx.recv_timeout(Duration::from_secs(10));
    let _ = child.kill();
    let _ = child.wait();
    result.context("Codex model query timed out")?
}

#[derive(Serialize)]
pub(crate) struct ModelCatalog {
    source: CatalogSource,
    models: Vec<ModelEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum CatalogSource {
    LatestAlias,
    InstalledCli,
    Unavailable,
}

#[derive(Serialize)]
struct ModelEntry {
    id: String,
    label: String,
}

impl ModelEntry {
    fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
        }
    }
}

pub(crate) fn models(provider: &str) -> Result<ModelCatalog> {
    let aliases = match provider {
        "claude" => vec!["fable", "opus", "sonnet", "haiku"],
        "gemini" => vec!["auto", "pro", "flash", "flash-lite"],
        _ => vec![],
    };
    if !aliases.is_empty() {
        return Ok(ModelCatalog {
            source: CatalogSource::LatestAlias,
            models: aliases
                .into_iter()
                .map(|alias| ModelEntry::new(alias, alias))
                .collect(),
        });
    }
    let mut items = Vec::new();
    if provider == "codex" {
        let catalog = codex_catalog()?;
        for model in catalog["data"].as_array().into_iter().flatten() {
            if let Some(id) = model["model"].as_str().or(model["id"].as_str()) {
                items.push(ModelEntry::new(
                    id,
                    model["displayName"].as_str().unwrap_or(id),
                ));
            }
        }
    } else if matches!(provider, "grok" | "agy" | "opencode") {
        let output = capture(provider_command(provider).arg("models"), 8)?;
        for line in output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            match provider {
                "grok" => {
                    if let Some(id) = line
                        .strip_prefix("- ")
                        .or(line.strip_prefix("* "))
                        .and_then(|text| text.split_whitespace().next())
                    {
                        items.push(ModelEntry::new(id, id));
                    }
                }
                "agy" => {
                    if let Some((id, label)) = line.split_once('\t') {
                        items.push(ModelEntry::new(id, label));
                    }
                }
                _ => items.push(ModelEntry::new(line, line)),
            }
        }
    }
    let source = if items.is_empty() {
        CatalogSource::Unavailable
    } else {
        CatalogSource::InstalledCli
    };
    Ok(ModelCatalog {
        source,
        models: items,
    })
}
// npm-based agent installations on Windows can expose a .cmd instead of an .exe.
// These programs and arguments are fixed by models(), never user-entered shell text.
fn provider_command(name: &str) -> Command {
    #[cfg(windows)]
    {
        let dirs = std::env::var_os("PATH").unwrap_or_default();
        for ext in ["exe", "cmd"] {
            for dir in std::env::split_paths(&dirs) {
                let path = dir.join(format!("{name}.{ext}"));
                if path.is_file() {
                    return command(path);
                }
            }
        }
    }
    command(name)
}
