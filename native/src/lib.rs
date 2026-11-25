mod api;
mod bots;
mod herdr;
pub mod ipc;
mod prompts;
mod store;
mod turns;

use anyhow::{Context, Result};
use fs2::FileExt;
use herdr::Herdr;
use rusqlite::Connection;
use serde_json::{json, Value as V};
use std::{
    collections::{HashMap, VecDeque},
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use store::Events;
use turns::{Job, Turn};

pub fn default_home() -> PathBuf {
    if let Some(home) = env::var_os("HERDR_BOT_HOME") {
        return PathBuf::from(home);
    }
    if let Ok(exe) = env::current_exe() {
        if exe.file_stem().is_some_and(|s| s == "herdr-bot")
            && exe.parent().is_some_and(|p| {
                p.ends_with("bin") && p.parent().is_some_and(|h| h.join("chat.sqlite3").exists())
            })
        {
            return exe.parent().unwrap().parent().unwrap().into();
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".herdr-bot")
}

pub struct Engine {
    db: Mutex<Connection>,
    herdr: Herdr,
    _lock: File,
    seq: AtomicU64,
    emit: Box<dyn Fn(V) + Send + Sync>,
    roster: Mutex<()>,
    active: Mutex<HashMap<String, Turn>>,
    queues: Mutex<HashMap<String, VecDeque<Job>>>,
    pub home: PathBuf,
    pub user: String,
}
impl Engine {
    pub fn start(
        home: PathBuf,
        cli: &Path,
        emit: impl Fn(V) + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        fs::create_dir_all(&home)?;
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(home.join("host.lock"))?;
        lock.try_lock_exclusive()
            .context("another herdr-bot host is using this data folder")?;
        let herdr = Herdr::new(&home);
        fs::create_dir_all(home.join("bin"))?;
        if !herdr.cli.exists() || !same_file_contents(cli, &herdr.cli)? {
            let mut tmp = tempfile::NamedTempFile::new_in(home.join("bin"))?;
            std::io::copy(
                &mut File::open(cli).with_context(|| format!("CLI missing: {}", cli.display()))?,
                &mut tmp,
            )?;
            tmp.as_file()
                .set_permissions(fs::metadata(cli)?.permissions())?;
            tmp.persist(&herdr.cli)?;
        }
        let user = env::var("HERDR_BOT_USER_NAME")
            .or_else(|_| env::var("USERNAME"))
            .or_else(|_| env::var("USER"))
            .unwrap_or("user".into());
        let engine = Arc::new(Self {
            db: Mutex::new(store::open(&home)?),
            herdr,
            home,
            user,
            _lock: lock,
            seq: AtomicU64::new(0),
            emit: Box::new(emit),
            roster: Mutex::default(),
            active: Mutex::default(),
            queues: Mutex::default(),
        });
        ipc::listen(&engine)?;
        let weak = Arc::downgrade(&engine);
        thread::spawn(move || {
            if let Some(e) = weak.upgrade() {
                let _ = e.herdr.ensure();
            }
            while let Some(e) = weak.upgrade() {
                let _ = e.refresh();
                drop(e);
                thread::sleep(Duration::from_secs(3));
            }
        });
        Ok(engine)
    }
    fn tx<T>(&self, f: impl FnOnce(&Connection, &mut Events) -> Result<T>) -> Result<T> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let mut events = Vec::new();
        let result = f(&tx, &mut events)?;
        tx.commit()?;
        for (kind, payload) in events {
            let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
            (self.emit)(json!({"seq":seq,"type":kind,"payload":payload}));
        }
        Ok(result)
    }
    pub fn call(self: &Arc<Self>, method: &str, p: V) -> Result<V> {
        self.dispatch(method, p)
    }
}
fn same_file_contents(a: &Path, b: &Path) -> Result<bool> {
    Ok(fs::metadata(a)?.len() == fs::metadata(b)?.len() && fs::read(a)? == fs::read(b)?)
}
pub fn wire_error(e: &anyhow::Error) -> V {
    let message = format!("{e:#}");
    let code = message
        .split_once(':')
        .map(|(code, _)| code)
        .filter(|c| c.chars().all(|x| x.is_ascii_lowercase() || x == '_'))
        .unwrap_or("internal");
    json!({"code":code,"message":message})
}
