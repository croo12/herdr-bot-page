mod chats;
mod entries;
mod inbox;
mod prompts;
mod reviews;
mod settings;
mod tasks;
mod transcript;

use crate::domain::Events;
use anyhow::Result;
pub(crate) use chats::Chats;
use inbox::Inbox;
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
    pub inbox: Inbox<'db>,
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
        let mut connection = Connection::open(home.join("chat.sqlite3"))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
    CREATE TABLE IF NOT EXISTS chats(id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS bots(id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS bot_requests(key TEXT PRIMARY KEY, bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE);
    CREATE TABLE IF NOT EXISTS entries(chat TEXT REFERENCES chats(id) ON DELETE CASCADE, seq INTEGER, data TEXT NOT NULL, PRIMARY KEY(chat,seq));
    CREATE UNIQUE INDEX IF NOT EXISTS direct_pair ON chats(json_extract(data,'$.directPair')) WHERE json_extract(data,'$.directPair') IS NOT NULL;
    CREATE TABLE IF NOT EXISTS config(key TEXT PRIMARY KEY,data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS inbox(
        bot TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE,
        chat TEXT NOT NULL, seq INTEGER NOT NULL, depth INTEGER NOT NULL,
        available INTEGER NOT NULL DEFAULT 0, state TEXT NOT NULL DEFAULT 'pending', last_error TEXT,
        PRIMARY KEY(bot,chat,seq), FOREIGN KEY(chat,seq) REFERENCES entries(chat,seq) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS inbox_ready ON inbox(available);
    UPDATE inbox SET state='pending',available=0 WHERE state='queued';
    UPDATE inbox SET state='unconfirmed' WHERE state='working';
    CREATE INDEX IF NOT EXISTS room_tasks ON entries(chat,seq) WHERE json_type(data,'$.task')='object';
    CREATE TABLE IF NOT EXISTS requests(key TEXT PRIMARY KEY, chat TEXT REFERENCES chats(id) ON DELETE CASCADE);
    CREATE TABLE IF NOT EXISTS permission_reviews(
        ticket TEXT PRIMARY KEY, chat TEXT NOT NULL, seq INTEGER NOT NULL, signature TEXT NOT NULL,
        target TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
        reviewer TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE, pane TEXT NOT NULL,
        UNIQUE(chat,seq), FOREIGN KEY(chat,seq) REFERENCES entries(chat,seq) ON DELETE CASCADE
    );")?;
        if connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? == 0 {
            let tx = connection.transaction()?;
            tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
            tx.execute_batch(include_str!("separate_bots.sql"))?;
            tx.commit()?;
        }
        connection.execute(
        "UPDATE chats SET data=json_set(data,'$.isRunning',json('false'),'$.currentActivity',null)
        WHERE json_extract(data,'$.isRunning')=1",
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
            inbox: Inbox::new(&tx),
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
    fn upgrades_bot_histories_without_replaying_them_and_preserves_review_tickets() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let old = Connection::open(dir.path().join("chat.sqlite3"))?;
        old.execute_batch("CREATE TABLE chats(id TEXT PRIMARY KEY,data TEXT NOT NULL);
        CREATE TABLE entries(chat TEXT REFERENCES chats(id),seq INTEGER,data TEXT NOT NULL,PRIMARY KEY(chat,seq));
        CREATE TABLE requests(key TEXT PRIMARY KEY,chat TEXT REFERENCES chats(id) ON DELETE CASCADE);
        CREATE TABLE permission_reviews(ticket TEXT PRIMARY KEY,chat TEXT,seq INTEGER,signature TEXT,target TEXT REFERENCES chats(id),reviewer TEXT REFERENCES chats(id),pane TEXT,FOREIGN KEY(chat,seq) REFERENCES entries(chat,seq));")?;
        for id in ["a", "b"] {
            let profile = crate::domain::new_chat(id, "bot", &json!({"name":id}));
            old.execute(
                "INSERT INTO chats VALUES(?,?)",
                rusqlite::params![id, profile],
            )?;
        }
        old.execute("INSERT INTO requests VALUES('create-a','a')", [])?;
        for (chat, author) in [("a", "a"), ("b", "a")] {
            old.execute(
                "INSERT INTO entries VALUES(?,1,?)",
                rusqlite::params![
                    chat,
                    json!({"id":"original","seq":1,"kind":"send-message","author":{"id":author}})
                ],
            )?;
        }
        old.execute(
            "INSERT INTO permission_reviews VALUES('t','b',1,'sig','b','a','p')",
            [],
        )?;
        drop(old);
        for _ in 0..2 {
            let db = Database::open(dir.path())?;
            db.transaction(
                |repos, events| {
                    let first = repos.chats.open_direct("a", None, events)?;
                    assert_eq!(first["id"], "history:a");
                    assert!(repos.transcript.contains("history:a", "original")?);
                    assert_eq!(repos.chats.get("history:b")?["archived"], true);
                    assert_ne!(
                        repos.chats.open_direct("b", None, events)?["id"],
                        "history:b"
                    );
                    assert!(repos.transcript.contains("history:b", "original")?);
                    assert!(repos.reviews.ticket_exists("t")?);
                    assert_eq!(
                        repos
                            .chats
                            .for_request(&json!({"requestId":"create-a"}))?
                            .unwrap()["id"],
                        "a"
                    );
                    assert!(repos.inbox.list("a")?.is_empty());
                    Ok(())
                },
                |_| {},
            )?;
            assert_eq!(
                db.0.lock().unwrap().query_row(
                    "SELECT count(*) FROM pragma_foreign_key_check",
                    [],
                    |row| row.get::<_, i64>(0)
                )?,
                0
            );
        }
        Ok(())
    }

    #[test]
    fn rich_mentions_route_to_stable_bot_ids_and_membership_removal_cancels_delivery() -> Result<()>
    {
        let dir = tempfile::tempdir()?;
        let db = Database::open(dir.path())?;
        db.transaction(|repos,events|{
            for bot in ["a","b"] {repos.chats.save(&crate::domain::new_chat(bot,"bot",&json!({"name":"같은 이름"})))?;}
            let mut room=crate::domain::new_chat("r","room",&json!({"memberIds":["a","b"]}));
            repos.chats.save(&room)?;
            let rich=json!({"content":[{"content":[{"type":"mention","attrs":{"id":"b","label":"같은 이름"}}]}]});
            let message=repos.append_entry("r",json!({"kind":"message","content":"@같은이름","richText":rich.to_string()}),events)?;
            repos.inbox.deliver(&room,&message,None,0,false)?;
            assert!(repos.inbox.list("a")?.is_empty());
            assert_eq!(repos.inbox.list("b")?[0]["messageId"],message["id"]);
            room["memberIds"]=json!(["a"]);
            repos.inbox.keep_members(&room)?;
            assert!(repos.inbox.list("b")?.is_empty());
            Ok(())
        }, |_|{})
    }

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
