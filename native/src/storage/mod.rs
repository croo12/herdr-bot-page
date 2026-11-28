mod bots;
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
use bots::Bots;
pub(crate) use bots::{BotSession, RegisteredBot};
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
    pub bots: Bots<'db>,
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
    CREATE INDEX IF NOT EXISTS entry_replies ON entries(chat,json_extract(data,'$.replyTo'));
    CREATE INDEX IF NOT EXISTS delegation_prompts ON entries(json_extract(data,'$.delegationId'))
        WHERE json_extract(data,'$.kind')='prompt' AND json_extract(data,'$.status')='pending';
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
        // Queued inbox deliveries survive a restart. Started deliveries need explicit confirmation.
        connection.execute(
            "UPDATE entries SET data=json_set(data,'$.delegation.state','interrupted','$.delegation.promptId',NULL,'$.delegation.promptChatId',NULL)
             WHERE json_extract(data,'$.delegation.state') IN ('queued','running','waiting')
               AND NOT EXISTS (
                   SELECT 1 FROM inbox JOIN entries AS message ON message.chat=inbox.chat AND message.seq=inbox.seq
                   WHERE inbox.state='pending' AND inbox.chat=json_extract(entries.data,'$.delegation.targetChatId')
                     AND (json_extract(message.data,'$.delegatedFrom.entryId')=json_extract(entries.data,'$.id')
                          OR (message.chat=entries.chat AND message.seq=entries.seq))
               )", [],
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
            bots: Bots::new(&tx),
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
    use crate::domain::MessageSource;
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
                        repos.bots.id_for_request("create-a")?,
                        Some(crate::domain::bot::BotId::from("a"))
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
            repos.inbox.deliver(&room,&message,MessageSource::User,0)?;
            assert!(repos.inbox.list("a")?.is_empty());
            assert_eq!(repos.inbox.list("b")?[0]["messageId"],message["id"]);
            room["memberIds"]=json!(["a"]);
            repos.inbox.keep_members(&room)?;
            assert!(repos.inbox.list("b")?.is_empty());
            Ok(())
        }, |_|{})
    }

    #[test]
    fn room_bot_replies_deliver_once_to_current_authors_and_mentions() -> Result<()> {
        let home = tempfile::tempdir()?;
        let db = Database::open(home.path())?;
        db.transaction(|repos, events| {
            for bot in ["alice","bob","carol"] {
                repos.chats.save(&crate::domain::new_chat(bot,"bot",&json!({"name":bot})))?;
            }
            let mut room = crate::domain::new_chat("room","room",&json!({"memberIds":["alice","bob","carol"]}));
            repos.chats.save(&room)?;
            let question = repos.append_entry("room",json!({"kind":"send-message","author":{"id":"alice"}}),events)?;
            let mut post = |room: &Value, parent: &Value, text: &str| -> Result<usize> {
                let reply = repos.append_entry("room",json!({"kind":"send-message","author":{"id":"bob"},"replyTo":parent,"message":{"type":"text","content":text}}),events)?;
                Ok(repos.inbox.deliver(room,&reply,MessageSource::Bot("bob"),2)?.len())
            };
            assert_eq!(post(&room,&question["id"],"@alice the build passed")?,1);
            assert_eq!(post(&room,&question["id"],"@carol please review")?,2);
            assert_eq!(post(&room,&Value::Null,"General update")?,0);
            let own_reply = repos.inbox.list("alice")?[0]["messageId"].clone();
            assert_eq!(post(&room,&own_reply,"Correction")?,0);
            room["memberIds"] = json!(["bob","carol"]);
            assert_eq!(post(&room,&question["id"],"Follow-up")?,0);
            assert_eq!(repos.inbox.list("alice")?.len(),2);
            assert_eq!(repos.inbox.list("carol")?.len(),1);
            assert!(repos.inbox.list("bob")?.is_empty());
            Ok(())
        }, |_| {})
    }

    #[test]
    fn a_delegation_keeps_its_pending_prompt_until_both_subscribers_finish() -> Result<()> {
        use crate::domain::{new_chat, DelegationRef};
        let home = tempfile::tempdir()?;
        Database::open(home.path())?.transaction(|repos, events| {
            for bot in ["alice", "bob"] {
                repos.chats.save(&new_chat(bot,"bot",&json!({"name":bot})))?;
            }
            let room = new_chat("room","room",&json!({"memberIds":["alice","bob"]}));
            repos.chats.save(&room)?;
            let request = repos.append_entry("room", json!({"kind":"send-message","delegation":{"state":"queued"}}), events)?;
            let tracking_id = request["id"].as_str().unwrap();
            let link = DelegationRef {chat_id:"room".into(),entry_id:tracking_id.into(),root_id:tracking_id.into()};
            repos.inbox.deliver(&room,&request,MessageSource::User,0)?;
            let start = |bot| -> Result<_> {
                let (_,job) = repos.inbox.reserve_next(bot)?.unwrap();
                let delivery = job.delivery.unwrap();
                assert!(repos.inbox.start(&delivery)?);
                Ok(delivery)
            };
            let alice = start("alice")?;
            let bob = start("bob")?;
            let mut prompt = repos.append_entry("room", json!({"kind":"prompt","author":{"id":"bob"},"delegationId":tracking_id,"status":"pending"}), events)?;
            repos.finish_delivery(&alice,Some(&link),events)?;
            let tracking = || -> Result<Value> {Ok(repos.transcript.by_id("room",tracking_id)?["delegation"].clone())};
            assert_eq!(tracking()?["state"],"waiting");
            assert_eq!(tracking()?["promptId"],prompt["id"]);
            prompt["status"]="answered".into();
            repos.update_entry("room",&prompt,events)?;
            repos.sync_delivery_state(&bob,Some(&link),events)?;
            assert_eq!(tracking()?["state"],"running");
            repos.finish_delivery(&bob,Some(&link),events)?;
            assert_eq!(tracking()?["state"],"completed");
            Ok(())
        }, |_| {})
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
