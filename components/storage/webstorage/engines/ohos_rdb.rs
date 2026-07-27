/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(ohos_rdb)]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use log::error;
use servo_base::threadpool::ThreadPool;
use tempfile::TempDir;

use crate::ohos_rdb::{
    OhosRdbError, OhosRdbStore, OhosRdbTransaction, OhosRdbValues, Result as OhosRdbResult,
};
use crate::webstorage::OriginEntry;

use super::WebStorageEngine;

const STORAGE_FILE_NAME: &str = "webstorage.rdb";

pub(crate) struct OhosRdbEngine {
    store: OhosRdbStore,
    _temp_dir: Option<TempDir>,
}

impl OhosRdbEngine {
    pub(crate) fn new(db_dir: &Option<PathBuf>, _pool: Arc<ThreadPool>) -> OhosRdbResult<Self> {
        let (db_dir, temp_dir) = match db_dir {
            Some(path) => (path.clone(), None),
            None => {
                let temp_dir = tempfile::tempdir()?;
                (temp_dir.path().to_path_buf(), Some(temp_dir))
            },
        };

        fs::create_dir_all(&db_dir)?;
        let store = OhosRdbStore::open(&db_dir, STORAGE_FILE_NAME)?;
        Self::init(&store)?;

        Ok(Self {
            store,
            _temp_dir: temp_dir,
        })
    }

    fn init(store: &OhosRdbStore) -> OhosRdbResult<()> {
        store.execute("PRAGMA foreign_keys = ON;")?;
        let tx = store.transaction()?;

        execute_no_args(
            &tx,
            r#"CREATE TABLE IF NOT EXISTS data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            key TEXT NOT NULL UNIQUE,
            value TEXT NOT NULL
        );"#,
        )?;

        tx.commit()?;
        Ok(())
    }

    fn save_inner(&mut self, data: &OriginEntry) -> OhosRdbResult<()> {
        let tx = self.store.transaction()?;
        execute_no_args(&tx, "DELETE FROM data;")?;

        for (key, value) in data.inner() {
            let mut args = OhosRdbValues::new()?;
            args.push_text(key)?;
            args.push_text(value)?;
            tx.execute("INSERT INTO data (key, value) VALUES (?1, ?2);", &args)?;
        }

        tx.commit()?;
        Ok(())
    }
}

impl WebStorageEngine for OhosRdbEngine {
    type Error = OhosRdbError;

    fn load(&self) -> Result<OriginEntry, Self::Error> {
        let tx = self.store.transaction()?;
        let args = OhosRdbValues::new()?;
        let mut cursor = tx.query_sql("SELECT key, value FROM data;", &args)?;

        let mut data = OriginEntry::default();
        while cursor.next_row()? {
            let key = required_text(&cursor, 0, "webstorage key lookup")?;
            let value = required_text(&cursor, 1, "webstorage value lookup")?;
            data.insert(key, value);
        }

        Ok(data)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        let tx = self.store.transaction()?;
        execute_no_args(&tx, "DELETE FROM data;")?;
        tx.commit()?;
        Ok(())
    }

    fn delete(&mut self, key: &str) -> Result<(), Self::Error> {
        let tx = self.store.transaction()?;
        let mut args = OhosRdbValues::new()?;
        args.push_text(key)?;
        tx.execute("DELETE FROM data WHERE key = ?1;", &args)?;
        tx.commit()?;
        Ok(())
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), Self::Error> {
        let tx = self.store.transaction()?;
        let mut args = OhosRdbValues::new()?;
        args.push_text(key)?;
        args.push_text(value)?;
        tx.execute("INSERT INTO data (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value;", &args)?;
        tx.commit()?;
        Ok(())
    }

    fn save(&mut self, data: &OriginEntry) {
        if let Err(error) = self.save_inner(data) {
            error!("localstorage save error: {:?}", error);
        }
    }
}

fn execute_no_args(tx: &OhosRdbTransaction, sql: &str) -> OhosRdbResult<()> {
    let args = OhosRdbValues::new()?;
    tx.execute(sql, &args)
}

fn required_text(
    cursor: &crate::ohos_rdb::OhosRdbCursor<'_>,
    index: usize,
    context: &'static str,
) -> OhosRdbResult<String> {
    cursor
        .text(index)?
        .ok_or(OhosRdbError::Api { context, code: -1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use servo_base::threadpool::ThreadPool;

    #[test]
    #[cfg(ohos_rdb)]
    fn test_ohos_webstorage_uses_rdb_file() {
        let tmp_dir = tempfile::tempdir().unwrap();

        let engine = OhosRdbEngine::new(&Some(tmp_dir.path().to_path_buf()), ThreadPool::global())
            .unwrap();

        assert!(tmp_dir.path().join("rdb").join(STORAGE_FILE_NAME).exists());
        drop(engine);
    }
}
