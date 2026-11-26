mod chats;
mod prompts;
mod reviews;
mod settings;
mod tasks;
mod transcript;

use crate::domain::Events;
use anyhow::Result;
use chats::Chats;
use prompts::Prompts;
use reviews::Reviews;
pub(crate) use reviews::{AssignedReview, UnassignedReview};
use rusqlite::Connection;
use serde_json::{json, Value};
use settings::Settings;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};
use tasks::Tasks;
use transcript::Transcript;

pub(crate) struct Store {
    db: Database,
    sequence: AtomicU64,
    emit: Box<dyn Fn(Value) + Send + Sync>,
}

impl Store {
    pub(crate) fn open(home: &Path, emit: impl Fn(Value) + Send + Sync + 'static) -> Result<Self> {
        Ok(Self {
            db: Database::open(home)?,
            sequence: AtomicU64::new(0),
            emit: Box::new(emit),
        })
    }

    pub(crate) fn tx<Output>(
        &self,
        work: impl FnOnce(&Repositories, &mut Events) -> Result<Output>,
    ) -> Result<Output> {
        self.db.transaction(work, |events| {
            for (kind, payload) in events {
                let seq = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
                (self.emit)(json!({"seq":seq,"type":kind,"payload":payload}));
            }
        })
    }
}

pub(crate) struct Repositories<'db> {
    pub chats: Chats<'db>,
    pub settings: Settings<'db>,
    pub tasks: Tasks<'db>,
    pub transcript: Transcript<'db>,
    pub prompts: Prompts<'db>,
    pub reviews: Reviews<'db>,
}
struct Database(Mutex<Connection>);
impl Database {
    fn open(home: &Path) -> Result<Self> {
        let connection = Connection::open(home.join("chat.sqlite3"))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
    CREATE TABLE IF NOT EXISTS chats(id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS entries(chat TEXT REFERENCES chats(id) ON DELETE CASCADE, seq INTEGER, data TEXT NOT NULL, PRIMARY KEY(chat,seq));
    CREATE TABLE IF NOT EXISTS config(key TEXT PRIMARY KEY,data TEXT NOT NULL);
    CREATE INDEX IF NOT EXISTS room_tasks ON entries(chat,seq) WHERE json_type(data,'$.task')='object';
    CREATE TABLE IF NOT EXISTS requests(key TEXT PRIMARY KEY, chat TEXT REFERENCES chats(id) ON DELETE CASCADE);
    CREATE TABLE IF NOT EXISTS permission_reviews(
        ticket TEXT PRIMARY KEY, chat TEXT NOT NULL, seq INTEGER NOT NULL, signature TEXT NOT NULL,
        target TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
        reviewer TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE, pane TEXT NOT NULL,
        UNIQUE(chat,seq), FOREIGN KEY(chat,seq) REFERENCES entries(chat,seq) ON DELETE CASCADE
    );")?;
        connection.execute(
        "UPDATE chats SET data=json_set(data,'$.isRunning',json('false'),'$.currentActivity',null)
        WHERE json_extract(data,'$.kind')='room' AND json_extract(data,'$.isRunning')=1",
        [],
    )?;
        Ok(Self(Mutex::new(connection)))
    }
    fn transaction<Output>(
        &self,
        work: impl FnOnce(&Repositories<'_>, &mut Events) -> Result<Output>,
        publish: impl FnOnce(Events),
    ) -> Result<Output> {
        let mut db = self.0.lock().unwrap();
        let tx = db.transaction()?;
        let repositories = Repositories {
            chats: Chats::new(&tx),
            settings: Settings::new(&tx),
            tasks: Tasks::new(&tx),
            transcript: Transcript::new(&tx),
            prompts: Prompts::new(&tx),
            reviews: Reviews::new(&tx),
        };
        let mut events = Events::new();
        let result = work(&repositories, &mut events)?;
        tx.commit()?;
        // Hold the writer lock until these events have been published in commit order.
        publish(events);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_and_request_commit_together_before_events_are_published() -> Result<()> {
        let home = tempfile::tempdir()?;
        let db = Database::open(home.path())?;
        let chat = json!({"id": "room-1", "kind": "room"});
        let request = json!({"requestId": "retry-1"});
        let failed: Result<()> = db.transaction(
            |repos, events| {
                repos.chats.save(&chat)?;
                repos.chats.remember(&request, &chat)?;
                events.push(("chat", chat.clone()));
                anyhow::bail!("abort transaction")
            },
            |_| panic!("published an event for a rolled-back chat"),
        );
        assert!(failed.is_err());
        db.transaction(
            |repos, events| {
                assert!(repos.chats.list()?.is_empty());
                assert!(repos.chats.for_request(&request)?.is_none());
                repos.chats.save(&chat)?;
                repos.chats.remember(&request, &chat)?;
                events.push(("chat", chat.clone()));
                Ok(())
            },
            |events| {
                assert_eq!(events, vec![("chat", chat.clone())]);
                let reader = Database::open(home.path()).unwrap();
                reader
                    .transaction(
                        |repos, _| {
                            assert_eq!(repos.chats.for_request(&request)?, Some(chat.clone()));
                            Ok(())
                        },
                        |_| {},
                    )
                    .unwrap();
            },
        )
    }
}
