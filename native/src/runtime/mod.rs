mod queue;

use queue::BotWork;

use crate::{
    controller::{ipc, Controller, Source},
    infrastructure::{herdr::Herdr, request_log::RequestLog, AgentSessions},
    storage::Store,
};
use anyhow::{Context, Result};
use fs2::FileExt;
use serde_json::{json, Value};

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
pub struct Engine {
    store: Store,
    request_log: RequestLog,
    sessions: AgentSessions,
    _lock: File,
    bot_queues: Mutex<HashMap<String, VecDeque<BotWork>>>,
    active_chats: Mutex<HashMap<String, usize>>,
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
            store: Store::open(&home, emit)?,
            request_log: RequestLog::new(&home),
            sessions: AgentSessions::new(herdr),
            home,
            user,
            _lock: lock,
            bot_queues: Mutex::default(),
            active_chats: Mutex::default(),
        });
        ipc::listen(&engine.home, &engine, |host, method, params| {
            host.call_from(Source::Ipc, method, params)
        })?;
        let weak = Arc::downgrade(&engine);
        thread::spawn(move || {
            if let Some(engine) = weak.upgrade() {
                let _ = engine.sessions.herdr.ensure();
            }
            while let Some(engine) = weak.upgrade() {
                let _ = engine.recover_work();
                let _ = crate::application::refresh_prompts(&engine.store, &engine.sessions);
                drop(engine);
                thread::sleep(Duration::from_secs(3));
            }
        });
        Ok(engine)
    }
    pub fn call(self: &Arc<Self>, method: &str, params: Value) -> Result<Value> {
        self.call_from(Source::Desktop, method, params)
    }

    fn call_from(self: &Arc<Self>, source: Source, method: &str, params: Value) -> Result<Value> {
        Controller {
            store: &self.store,
            sessions: &self.sessions,
            user: &self.user,
            request_log: &self.request_log,
            wake_bot: &|bot| self.wake_bot(bot),
            enqueue_greeting: &|bot, chat, job| {
                self.enqueue(
                    bot,
                    BotWork::Greeting {
                        chat: chat.into(),
                        job,
                    },
                )
            },
            host_status: &|| self.host_status(),
        }
        .call_from(source, method, params)
    }

    fn host_status(&self) -> Result<Value> {
        self.store.tx(|repos, _| {
            let chats = repos.chats.list()?;
            let bots = chats.iter().filter(|chat| chat["kind"] == "bot").count();
            // The status API includes scheduled chats even before a newly woken bot claims its message.
            let mut running: BTreeSet<_> = repos.inbox.scheduled_chats()?.into_iter().collect();
            for chat in &chats {
                if chat["kind"] != "bot" && chat["isRunning"] == true {
                    running.insert(crate::domain::need(chat, "id")?.to_owned());
                }
            }
            Ok(json!({
                "home": self.home, "session": self.sessions.herdr.session, "bots": bots,
                "rooms": chats.iter().filter(|chat|chat["kind"]=="room").count(), "runningChatIds": running
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
