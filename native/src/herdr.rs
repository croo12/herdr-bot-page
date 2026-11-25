use crate::store::need;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value as V};
use std::{
    env,
    ffi::OsStr,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use wait_timeout::ChildExt;

pub fn command(file: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(file);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}
fn capture(cmd: &mut Command, seconds: u64) -> Result<String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let read = |mut stream: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut data = Vec::new();
            stream
                .read_to_end(&mut data)
                .map(|_| String::from_utf8_lossy(&data).into_owned())
        })
    };
    let out = read(Box::new(out));
    let err = read(Box::new(err));
    let Some(status) = child.wait_timeout(Duration::from_secs(seconds))? else {
        let _ = child.kill();
        let _ = child.wait();
        bail!("command timed out after {seconds}s")
    };
    let out = out.join().map_err(|_| anyhow!("stdout reader stopped"))??;
    let err = err.join().map_err(|_| anyhow!("stderr reader stopped"))??;
    if !status.success() {
        bail!(
            "{}",
            if err.trim().is_empty() {
                out.trim()
            } else {
                err.trim()
            }
        );
    }
    Ok(out)
}

#[derive(Clone)]
pub struct Herdr {
    pub bin: String,
    pub session: String,
    pub home: PathBuf,
    pub cli: PathBuf,
}
impl Herdr {
    pub fn new(home: &Path) -> Self {
        Self {
            bin: env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".into()),
            session: env::var("HERDR_BOT_SESSION").unwrap_or_else(|_| "herdr-bot".into()),
            home: home.into(),
            cli: home.join("bin").join(if cfg!(windows) {
                "herdr-bot.exe"
            } else {
                "herdr-bot"
            }),
        }
    }
    fn process(&self) -> Command {
        let mut c = command(&self.bin);
        c.args(["--session", &self.session])
            .env("HERDR_BOT_HOME", &self.home);
        c.env_remove("HERDR_SOCKET_PATH");
        c
    }
    pub fn text(&self, args: &[&str], seconds: u64) -> Result<String> {
        capture(self.process().args(args), seconds).with_context(|| {
            format!(
                "herdr_error: herdr {} {}",
                args[0],
                args.get(1).unwrap_or(&"")
            )
        })
    }
    pub fn run(&self, args: &[&str]) -> Result<V> {
        self.run_timeout(args, 45)
    }
    pub fn run_timeout(&self, args: &[&str], seconds: u64) -> Result<V> {
        let text = self.text(args, seconds)?;
        let v: V = serde_json::from_str(&text).context("herdr_error: invalid JSON from herdr")?;
        if !v["error"].is_null() {
            bail!("herdr_error: {}", v["error"]);
        }
        v.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("herdr_error: missing result"))
    }
    pub fn ensure(&self) -> Result<()> {
        let running = || -> Result<bool> {
            let v: V = serde_json::from_str(&self.text(&["session", "list", "--json"], 5)?)?;
            Ok(v["sessions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|a| a["name"] == self.session && a["running"] == true))
        };
        if running()? {
            return Ok(());
        }
        let mut cmd = self.process();
        cmd.arg("server")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000200);
        }
        let mut child = cmd.spawn()?;
        thread::spawn(move || {
            let _ = child.wait();
        });
        for _ in 0..40 {
            thread::sleep(Duration::from_millis(250));
            if running()? {
                return Ok(());
            }
        }
        bail!("herdr_error: session {} did not start", self.session)
    }
    pub fn agents(&self) -> Result<Vec<V>> {
        Ok(self.run(&["agent", "list"])?["agents"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }
    pub fn launch(&self, id: &str, bot: &V) -> Result<V> {
        let cwd = need(bot, "cwd")?;
        let kind = need(bot, "provider")?;
        let label = format!("herdr-bot: {cwd}");
        let workspaces = self.run(&["workspace", "list"])?;
        let existing = workspaces["workspaces"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|w| w["label"] == label);
        let location = if let Some(w) = existing {
            self.run(&[
                "tab",
                "create",
                "--workspace",
                need(w, "workspace_id")?,
                "--cwd",
                cwd,
                "--label",
                id,
                "--no-focus",
            ])?
        } else {
            self.run(&[
                "workspace",
                "create",
                "--cwd",
                cwd,
                "--label",
                &label,
                "--no-focus",
            ])?
        };
        let pane = need(&location["root_pane"], "pane_id")?;
        let mut opts = Vec::<String>::new();
        if kind == "claude" && bot["permissionMode"] == "auto" {
            opts.extend(["--permission-mode".into(), "bypassPermissions".into()]);
        }
        if kind == "codex" && bot["permissionMode"] == "auto" {
            opts.extend(["-s", "workspace-write", "-a", "on-request"].map(String::from));
        }
        if let Some(model) = bot["model"].as_str() {
            opts.extend(["--model".into(), model.into()]);
        }
        if let Some(e) = bot["reasoningEffort"].as_str() {
            match kind {
                "claude" => opts.extend(["--effort".into(), e.into()]),
                "grok" => opts.extend(["--reasoning-effort".into(), e.into()]),
                "codex" => {
                    opts.extend(["--config".into(), format!("model_reasoning_effort=\"{e}\"")])
                }
                _ => {}
            }
        }
        let mut args = vec![
            "agent",
            "start",
            id,
            "--kind",
            kind,
            "--pane",
            pane,
            "--timeout",
            "30000",
        ];
        if !opts.is_empty() {
            args.push("--");
            args.extend(opts.iter().map(String::as_str));
        }
        match self.run(&args) {
            Ok(v) => Ok(v["agent"].clone()),
            Err(e) => {
                // First launch can stop at a provider's sign-in or trust screen.
                // Keep that pane reachable from the UI so the user can finish setup.
                if let Ok(live) = self.run(&["agent", "get", pane]) {
                    let agent = &live["agent"];
                    if agent["pane_id"] == pane
                        && agent["agent"] == kind
                        && agent["agent_status"] == "blocked"
                    {
                        if agent["name"] != id {
                            self.run(&["agent", "rename", pane, id])?;
                        }
                        return Ok(agent.clone());
                    }
                }
                let _ = self.run(&["pane", "close", pane]);
                Err(e)
            }
        }
    }
    pub fn focus(&self, pane: &str) -> Result<()> {
        self.run(&["agent", "focus", pane])?;
        #[cfg(windows)]
        {
            let mut c = Command::new(&self.bin);
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x10);
            for (k, _) in env::vars_os() {
                if k.to_string_lossy().starts_with("HERDR_") {
                    c.env_remove(k);
                }
            }
            c.args(["session", "attach", &self.session]).spawn()?;
        }
        #[cfg(target_os = "macos")]
        {
            fn quote(s: &str) -> String {
                format!("'{}'", s.replace('\'', "'\\''"))
            }
            let line = format!(
                "{} session attach {}",
                quote(&self.bin),
                quote(&self.session)
            );
            command("osascript")
                .args([
                    "-e",
                    &format!(
                        "tell application \"Terminal\" to do script {}",
                        serde_json::to_string(&line)?
                    ),
                ])
                .status()?;
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            command("x-terminal-emulator")
                .args(["-e", &self.bin, "session", "attach", &self.session])
                .spawn()?;
        }
        Ok(())
    }
}
fn codex_catalog() -> Result<V> {
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
        let result = (|| -> Result<V> {
            let init = json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"herdr-bot","version":"1.3.0"}}});
            writeln!(input, "{init}")?;
            for line in BufReader::new(output).lines() {
                let Ok(frame) = serde_json::from_str::<V>(&line?) else {
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

pub fn models(provider: &str) -> Result<V> {
    let aliases = match provider {
        "claude" => vec!["fable", "opus", "sonnet", "haiku"],
        "gemini" => vec!["auto", "pro", "flash", "flash-lite"],
        _ => vec![],
    };
    let entry = |id: &str, label: &str| json!({"id":id,"label":label});
    if !aliases.is_empty() {
        return Ok(
            json!({"source":"latest-alias","models":aliases.into_iter().map(|x|entry(x,x)).collect::<Vec<_>>()}),
        );
    }
    let mut items = Vec::new();
    if provider == "codex" {
        let catalog = codex_catalog()?;
        for m in catalog["data"].as_array().into_iter().flatten() {
            if let Some(id) = m["model"].as_str().or(m["id"].as_str()) {
                items.push(entry(id, m["displayName"].as_str().unwrap_or(id)));
            }
        }
    } else if matches!(provider, "grok" | "agy" | "opencode") {
        let output = capture(provider_command(provider).arg("models"), 8)?;
        for line in output.lines().map(str::trim).filter(|l| !l.is_empty()) {
            match provider {
                "grok" => {
                    if let Some(id) = line
                        .strip_prefix("- ")
                        .or(line.strip_prefix("* "))
                        .and_then(|x| x.split_whitespace().next())
                    {
                        items.push(entry(id, id));
                    }
                }
                "agy" => {
                    if let Some((id, label)) = line.split_once('\t') {
                        items.push(entry(id, label));
                    }
                }
                _ => items.push(entry(line, line)),
            }
        }
    }
    Ok(json!({"models":items,"source":if items.is_empty(){"unavailable"}else{"installed-cli"}}))
}
// npm-based agent installations on Windows can expose a .cmd instead of an .exe.
// These programs and arguments are fixed by models(), never user-entered shell text.
fn provider_command(name: &str) -> Command {
    #[cfg(windows)]
    {
        let dirs = env::var_os("PATH").unwrap_or_default();
        for ext in ["exe", "cmd"] {
            for dir in env::split_paths(&dirs) {
                let path = dir.join(format!("{name}.{ext}"));
                if path.is_file() {
                    return command(path);
                }
            }
        }
    }
    command(name)
}
