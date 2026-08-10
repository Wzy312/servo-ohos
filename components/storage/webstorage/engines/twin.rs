/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Both webstorage backends in one binary, selected per origin. See
//! [`crate::indexeddb::engines::twin`] for the IndexedDB counterpart.

use std::path::PathBuf;
use std::sync::Arc;

use servo_base::threadpool::ThreadPool;

use super::WebStorageEngine;
use super::ohos_rdb::OhosRdbEngine;
use super::sqlite::SqliteEngine;
use crate::shared::{TwinError, use_ohos_rdb_backend};
use crate::webstorage::OriginEntry;

pub(crate) enum TwinEngine {
    Sqlite(SqliteEngine),
    Rdb(OhosRdbEngine),
}

impl TwinEngine {
    pub(crate) fn new(db_dir: &Option<PathBuf>, pool: Arc<ThreadPool>) -> Result<Self, TwinError> {
        if use_ohos_rdb_backend() {
            OhosRdbEngine::new(db_dir, pool)
                .map(TwinEngine::Rdb)
                .map_err(TwinError::Rdb)
        } else {
            SqliteEngine::new(db_dir, pool)
                .map(TwinEngine::Sqlite)
                .map_err(TwinError::Sqlite)
        }
    }
}

impl WebStorageEngine for TwinEngine {
    type Error = TwinError;

    fn load(&self) -> Result<OriginEntry, Self::Error> {
        match self {
            TwinEngine::Sqlite(engine) => engine.load().map_err(TwinError::Sqlite),
            TwinEngine::Rdb(engine) => engine.load().map_err(TwinError::Rdb),
        }
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        match self {
            TwinEngine::Sqlite(engine) => engine.clear().map_err(TwinError::Sqlite),
            TwinEngine::Rdb(engine) => engine.clear().map_err(TwinError::Rdb),
        }
    }

    fn delete(&mut self, key: &str) -> Result<(), Self::Error> {
        match self {
            TwinEngine::Sqlite(engine) => engine.delete(key).map_err(TwinError::Sqlite),
            TwinEngine::Rdb(engine) => engine.delete(key).map_err(TwinError::Rdb),
        }
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), Self::Error> {
        match self {
            TwinEngine::Sqlite(engine) => engine.set(key, value).map_err(TwinError::Sqlite),
            TwinEngine::Rdb(engine) => engine.set(key, value).map_err(TwinError::Rdb),
        }
    }

    fn save(&mut self, data: &OriginEntry) {
        match self {
            TwinEngine::Sqlite(engine) => engine.save(data),
            TwinEngine::Rdb(engine) => engine.save(data),
        }
    }
}
