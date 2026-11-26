use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

pub(crate) struct UnassignedReview {
    pub chat_id: String,
    pub entry: Value,
    pub requester: Value,
    pub room: Value,
}

pub(crate) struct AssignedReview {
    pub chat_id: String,
    pub entry: Value,
    pub requester: Value,
    pub reviewer: Value,
}

pub(crate) struct Reviews<'db> {
    db: &'db Connection,
}
impl<'db> Reviews<'db> {
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }
    pub(crate) fn pending_for(&self, reviewer: &str) -> Result<bool> {
        Ok(self.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM permission_reviews r JOIN entries e ON e.chat=r.chat AND e.seq=r.seq
         WHERE r.reviewer=? AND json_extract(e.data,'$.status')='pending' AND json_extract(e.data,'$.permissionReview.decision')='pending')",
        [reviewer], |row| row.get(0),
    )?)
    }
    pub(crate) fn ticket_exists(&self, ticket: &str) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM permission_reviews WHERE ticket=?)",
            [ticket],
            |row| row.get(0),
        )?)
    }
    pub(crate) fn pending_in_room(&self, room_id: &str) -> Result<Vec<(String, String, Value)>> {
        let mut statement = self.db.prepare("SELECT r.target,r.reviewer,e.data FROM permission_reviews r JOIN entries e ON e.chat=r.chat AND e.seq=r.seq WHERE r.chat=? AND json_extract(e.data,'$.status')='pending'")?;
        let rows = statement
            .query_map([room_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Value>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    pub(crate) fn revoke(&self, room: &str, seq: &Value) -> Result<()> {
        self.db.execute(
            "DELETE FROM permission_reviews WHERE chat=? AND seq=?",
            params![room, seq],
        )?;
        Ok(())
    }
    pub(crate) fn unassigned(&self) -> Result<Vec<UnassignedReview>> {
        let mut statement = self.db.prepare("SELECT e.chat,e.data,b.data,room.data FROM entries e JOIN chats room ON room.id=e.chat
                JOIN chats b ON b.id=json_extract(e.data,'$.author.id')
                WHERE json_extract(room.data,'$.kind')='room'
                AND json_extract(e.data,'$.status')='pending' AND json_extract(e.data,'$.prompt.kind')='permission'
                AND json_extract(e.data,'$.permissionReview') IS NULL")?;
        let rows = statement.query_map([], |row| {
            Ok(UnassignedReview {
                chat_id: row.get(0)?,
                entry: row.get(1)?,
                requester: row.get(2)?,
                room: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub(crate) fn assign(
        &self,
        ticket: &str,
        chat: &str,
        entry: &Value,
        target: &str,
        reviewer: &str,
        pane: &str,
    ) -> Result<()> {
        self.db.execute(
            "INSERT INTO permission_reviews(ticket,chat,seq,signature,target,reviewer,pane)
             VALUES(?,?,?,?,?,?,?)",
            params![
                ticket,
                chat,
                entry["seq"],
                entry["prompt"]["signature"].as_str(),
                target,
                reviewer,
                pane
            ],
        )?;
        Ok(())
    }
    pub(crate) fn assignment(
        &self,
        ticket: &str,
        pane: &str,
        chat: &str,
    ) -> Result<Option<AssignedReview>> {
        Ok(self.db.query_row("SELECT r.chat,e.data,b.data,a.data FROM permission_reviews r
                JOIN entries e ON e.chat=r.chat AND e.seq=r.seq JOIN chats b ON b.id=r.target JOIN chats a ON a.id=r.reviewer JOIN chats room ON room.id=e.chat
                WHERE r.ticket=? AND r.pane=? AND json_extract(a.data,'$.bot.paneId')=r.pane
                AND r.chat=? AND json_extract(room.data,'$.kind')='room' AND r.reviewer!=r.target
                AND r.reviewer IN (SELECT value FROM json_each(room.data,'$.memberIds'))
                AND r.target IN (SELECT value FROM json_each(room.data,'$.memberIds'))
                AND json_extract(e.data,'$.status')='pending' AND json_extract(e.data,'$.permissionReview.decision')='pending'
                AND json_extract(e.data,'$.prompt.signature')=r.signature",
                params![ticket, pane, chat],
                |row| Ok(AssignedReview {
                    chat_id: row.get(0)?,
                    entry: row.get(1)?,
                    requester: row.get(2)?,
                    reviewer: row.get(3)?,
                }),
            ).optional()?)
    }
}
