use crate::{
    controller::ipc,
    domain::Events,
    infrastructure::{herdr::Herdr, request_log::RequestLog},
    modules::turns::{QueueItem, Turn},
    storage::{Database, Repositories},
};
use anyhow::{Context, Result};
use fs2::FileExt;
use serde_json::{json, Value};

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
pub struct Engine {
    db: Database,
    pub(crate) request_log: RequestLog,
    pub(crate) herdr: Herdr,
    _lock: File,
    seq: AtomicU64,
    emit: Box<dyn Fn(Value) + Send + Sync>,
    pub(crate) roster: Mutex<()>,
    pub(crate) active: Mutex<HashMap<String, Turn>>,
    pub(crate) queues: Mutex<HashMap<String, VecDeque<QueueItem>>>,
    pub home: PathBuf,
    pub user: String,
}
impl Engine {
    pub fn start(
        home: PathBuf,
        cli: &Path,
        emit: impl Fn(Value) + Send + Sync + 'static,
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
            db: Database::open(&home)?,
            request_log: RequestLog::new(&home),
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
            if let Some(engine) = weak.upgrade() {
                let _ = engine.herdr.ensure();
            }
            while let Some(engine) = weak.upgrade() {
                let _ = crate::modules::tasks::dispatch(&engine);
                let _ = crate::modules::prompts::refresh(&engine);
                drop(engine);
                thread::sleep(Duration::from_secs(3));
            }
        });
        Ok(engine)
    }
    pub(crate) fn tx<Output>(
        &self,
        work: impl FnOnce(&Repositories, &mut Events) -> Result<Output>,
    ) -> Result<Output> {
        self.db.transaction(work, |events| {
            for (kind, payload) in events {
                let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
                (self.emit)(json!({"seq":seq,"type":kind,"payload":payload}));
            }
        })
    }
    pub(crate) fn host_status(&self) -> Result<Value> {
        self.tx(|repos, _| {
            let chats = repos.chats.list()?;
            let bots = chats.iter().filter(|chat| chat["kind"] == "bot").count();
            let running: Vec<_> = self.queues.lock().unwrap().keys().cloned().collect();
            Ok(json!({
                "home": self.home, "session": self.herdr.session, "bots": bots,
                "rooms": chats.len() - bots, "runningChatIds": running
            }))
        })
    }
}
fn same_file_contents(source: &Path, destination: &Path) -> Result<bool> {
    Ok(
        fs::metadata(source)?.len() == fs::metadata(destination)?.len()
            && fs::read(source)? == fs::read(destination)?,
    )
}
