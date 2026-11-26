use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

pub(crate) struct Settings<'db> {
    db: &'db Connection,
}
impl<'db> Settings<'db> {
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }
    pub(crate) fn get(&self) -> Result<Option<Value>> {
        Ok(self
            .db
            .query_row("SELECT data FROM config WHERE key='settings'", [], |row| {
                row.get(0)
            })
            .optional()?)
    }
    pub(crate) fn save(&self, value: &Value) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO config VALUES('settings',?)",
            [value],
        )?;
        Ok(())
    }
}
