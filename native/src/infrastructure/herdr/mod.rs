use crate::domain::{
    bot::{ClaudePermissionMode, CodexPermissionMode, Provider},
    need,
};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::{
    env,
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use wait_timeout::ChildExt;

fn parse_agent_list(mut response: Value) -> Result<Vec<Value>> {
    match response.get_mut("agents").map(Value::take) {
        Some(Value::Array(agents)) => Ok(agents),
        _ => bail!("herdr_error: agent list has no agents array"),
    }
}

fn launch_options(provider: &Provider) -> Vec<String> {
    let mut options = Vec::new();
    let (model, effort_option) = match provider {
        Provider::Claude {
            model,
            permission_mode,
            reasoning_effort,
        } => {
            if *permission_mode == ClaudePermissionMode::BypassPermissions {
                options.extend(["--permission-mode", "bypassPermissions"].map(String::from));
            }
            (
                model.as_ref().map(|id| &id.0),
                reasoning_effort
                    .as_ref()
                    .map(|effort| ("--effort", effort.as_str().to_owned())),
            )
        }
        Provider::Codex {
            model,
            permission_mode,
            reasoning_effort,
        } => {
            if *permission_mode == CodexPermissionMode::WorkspaceWriteOnRequest {
                options.extend(["-s", "workspace-write", "-a", "on-request"].map(String::from));
            }
            (
                model.as_ref().map(|id| &id.0),
                reasoning_effort.as_ref().map(|effort| {
                    (
                        "--config",
                        format!("model_reasoning_effort=\"{}\"", effort.as_str()),
                    )
                }),
            )
        }
        Provider::Grok {
            model,
            reasoning_effort,
        } => (
            model.as_ref().map(|id| &id.0),
            reasoning_effort
                .as_ref()
                .map(|effort| ("--reasoning-effort", effort.as_str().to_owned())),
        ),
        Provider::Gemini { model } => (model.as_ref().map(|id| &id.0), None),
        Provider::Antigravity { model } => (model.as_ref().map(|id| &id.0), None),
        Provider::OpenCode { model } => (model.as_ref().map(|id| &id.0), None),
        Provider::Other(_) => (None, None),
    };
    if let Some(model) = model {
        options.extend(["--model".into(), model.clone()]);
    }
    if let Some((flag, value)) = effort_option {
        options.extend([flag.into(), value]);
    }
    options
}

#[derive(Debug)]
struct HerdrFailure {
    code: String,
    message: String,
}

impl HerdrFailure {
    fn from_json(error: &Value) -> Option<Self> {
        Some(Self {
            code: error["code"].as_str()?.to_owned(),
            message: error["message"].as_str()?.to_owned(),
        })
    }
}

impl std::fmt::Display for HerdrFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HerdrFailure {}

pub(crate) fn has_error_code(error: &anyhow::Error, code: &str) -> bool {
    error
        .downcast_ref::<HerdrFailure>()
        .is_some_and(|failure| failure.code == code)
}

pub fn command(file: impl AsRef<OsStr>) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new(file);
        cmd.creation_flags(0x08000000);
        cmd
    }
    #[cfg(not(windows))]
    {
        Command::new(file)
    }
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
        let detail = if err.trim().is_empty() {
            out.trim()
        } else {
            err.trim()
        };
        if let Ok(frame) = serde_json::from_str::<Value>(detail) {
            if let Some(failure) = HerdrFailure::from_json(&frame["error"]) {
                return Err(failure.into());
            }
        }
        bail!("{detail}");
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
        let mut process = command(&self.bin);
        process
            .args(["--session", &self.session])
            .env("HERDR_BOT_HOME", &self.home);
        process.env_remove("HERDR_SOCKET_PATH");
        process
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
    pub fn run(&self, args: &[&str]) -> Result<Value> {
        self.run_timeout(args, 45)
    }
    pub fn run_timeout(&self, args: &[&str], seconds: u64) -> Result<Value> {
        let text = self.text(args, seconds)?;
        let response: Value =
            serde_json::from_str(&text).context("herdr_error: invalid JSON from herdr")?;
        if !response["error"].is_null() {
            if let Some(failure) = HerdrFailure::from_json(&response["error"]) {
                return Err(
                    anyhow!(failure).context(format!("herdr_error: herdr {} {}", args[0], args[1]))
                );
            }
            bail!("herdr_error: {}", response["error"]);
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("herdr_error: missing result"))
    }
    pub fn ensure(&self) -> Result<()> {
        let running = || -> Result<bool> {
            let response: Value =
                serde_json::from_str(&self.text(&["session", "list", "--json"], 5)?)?;
            Ok(response["sessions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|session| session["name"] == self.session && session["running"] == true))
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
    pub fn ensure_and_list_agents(&self) -> Result<Vec<Value>> {
        self.ensure()?;
        self.agents()
    }

    pub fn agents(&self) -> Result<Vec<Value>> {
        parse_agent_list(self.run(&["agent", "list"])?)
    }
    pub fn launch(&self, id: &str, directory: &Path, provider: &Provider) -> Result<Value> {
        let cwd = directory
            .to_str()
            .filter(|cwd| !cwd.trim().is_empty())
            .context("invalid_args: missing cwd")?;
        let kind = provider.name();
        let opts = launch_options(provider);
        let label = format!("herdr-bot: {cwd}");
        let workspaces = self.run(&["workspace", "list"])?;
        let existing = workspaces["workspaces"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|workspace| workspace["label"] == label);
        let location = if let Some(workspace) = existing {
            self.run(&[
                "tab",
                "create",
                "--workspace",
                need(workspace, "workspace_id")?,
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
            Ok(response) => Ok(response["agent"].clone()),
            Err(error) => {
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
                Err(error)
            }
        }
    }
    pub fn focus(&self, pane: &str) -> Result<()> {
        self.run(&["agent", "focus", pane])?;
        #[cfg(windows)]
        {
            let mut process = Command::new(&self.bin);
            use std::os::windows::process::CommandExt;
            process.creation_flags(0x10);
            for (key, _) in env::vars_os() {
                if key.to_string_lossy().starts_with("HERDR_") {
                    process.env_remove(key);
                }
            }
            process.args(["session", "attach", &self.session]).spawn()?;
        }
        #[cfg(target_os = "macos")]
        {
            fn quote(argument: &str) -> String {
                format!("'{}'", argument.replace('\'', "'\\''"))
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

mod catalog;
pub(crate) use catalog::models;
