use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;

pub(crate) struct Tasks<'db> {
    db: &'db Connection,
}

impl<'db> Tasks<'db> {
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }

    pub(crate) fn runnable(&self) -> Result<Vec<(String, String, String)>> {
        let mut statement = self.db.prepare(
            "SELECT chat,json_extract(data,'$.id'),json_extract(data,'$.task.assigneeId') FROM entries
             WHERE json_type(data,'$.task')='object'
             AND json_type(data,'$.task.assigneeId')='text' AND json_extract(data,'$.task.execution.state') IN ('queued','running')
             ORDER BY json_extract(data,'$.task.updatedAt'),seq",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn list(&self, room: &str) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare(
            "SELECT data FROM entries WHERE chat=? AND json_type(data,'$.task')='object' ORDER BY seq",
        )?;
        let entries = statement.query_map([room], |row| row.get(0))?;
        Ok(entries.collect::<rusqlite::Result<_>>()?)
    }
}
