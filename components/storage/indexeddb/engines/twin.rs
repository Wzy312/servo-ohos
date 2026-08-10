/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Both IndexedDB backends in one binary, selected per database open, so that
//! the SQLite and the OHOS RDB engine can be compared without reinstalling.

use std::path::PathBuf;
use std::sync::Arc;

use malloc_size_of_derive::MallocSizeOf;
use servo_base::threadpool::ThreadPool;
use storage_traits::indexeddb::{CreateObjectResult, IndexedDBIndex, KeyPath};

use super::ohos_rdb::OhosRdbEngine;
use super::sqlite::SqliteEngine;
use super::{KvsEngine, KvsTransaction};
use crate::indexeddb::IndexedDBDescription;
use crate::shared::{TwinError, use_ohos_rdb_backend};

#[derive(MallocSizeOf)]
pub(crate) enum TwinEngine {
    Sqlite(SqliteEngine),
    Rdb(OhosRdbEngine),
}

/// Delegate a `Result`-returning [`KvsEngine`] method to whichever engine is active.
macro_rules! delegate {
    ($self:expr, |$engine:ident| $call:expr) => {
        match $self {
            TwinEngine::Sqlite($engine) => $call.map_err(TwinError::Sqlite),
            TwinEngine::Rdb($engine) => $call.map_err(TwinError::Rdb),
        }
    };
}

impl TwinEngine {
    pub(crate) fn new(
        path: PathBuf,
        created: bool,
        db_info: &IndexedDBDescription,
        pool: Arc<ThreadPool>,
    ) -> Result<Self, TwinError> {
        if use_ohos_rdb_backend() {
            OhosRdbEngine::new(path, created, db_info, pool)
                .map(TwinEngine::Rdb)
                .map_err(TwinError::Rdb)
        } else {
            SqliteEngine::new(path, created, db_info, pool)
                .map(TwinEngine::Sqlite)
                .map_err(TwinError::Sqlite)
        }
    }

    pub(crate) fn created_db_path(&self) -> bool {
        match self {
            TwinEngine::Sqlite(engine) => engine.created_db_path(),
            TwinEngine::Rdb(engine) => engine.created_db_path(),
        }
    }
}

impl KvsEngine for TwinEngine {
    type Error = TwinError;

    fn create_store(
        &self,
        store_name: &str,
        key_path: Option<KeyPath>,
        auto_increment: bool,
    ) -> Result<CreateObjectResult, Self::Error> {
        delegate!(self, |engine| engine.create_store(
            store_name,
            key_path,
            auto_increment
        ))
    }

    fn delete_store(&self, store_name: &str) -> Result<(), Self::Error> {
        delegate!(self, |engine| engine.delete_store(store_name))
    }

    fn close_store(&self, store_name: &str) -> Result<(), Self::Error> {
        delegate!(self, |engine| engine.close_store(store_name))
    }

    fn process_transaction(
        &self,
        transaction: KvsTransaction,
        on_complete: Box<dyn FnOnce() + Send + 'static>,
    ) {
        match self {
            TwinEngine::Sqlite(engine) => engine.process_transaction(transaction, on_complete),
            TwinEngine::Rdb(engine) => engine.process_transaction(transaction, on_complete),
        }
    }

    fn key_generator_current_number(&self, store_name: &str) -> Option<i64> {
        match self {
            TwinEngine::Sqlite(engine) => engine.key_generator_current_number(store_name),
            TwinEngine::Rdb(engine) => engine.key_generator_current_number(store_name),
        }
    }

    fn set_key_generator_current_number(
        &self,
        store_name: &str,
        current_number: i64,
    ) -> Result<(), Self::Error> {
        delegate!(self, |engine| engine
            .set_key_generator_current_number(store_name, current_number))
    }

    fn key_path(&self, store_name: &str) -> Option<KeyPath> {
        match self {
            TwinEngine::Sqlite(engine) => engine.key_path(store_name),
            TwinEngine::Rdb(engine) => engine.key_path(store_name),
        }
    }

    fn object_store_names(&self) -> Result<Vec<String>, Self::Error> {
        delegate!(self, |engine| engine.object_store_names())
    }

    fn indexes(&self, store_name: &str) -> Result<Vec<IndexedDBIndex>, Self::Error> {
        delegate!(self, |engine| engine.indexes(store_name))
    }

    fn create_index(
        &self,
        store_name: &str,
        index_name: String,
        key_path: KeyPath,
        unique: bool,
        multi_entry: bool,
    ) -> Result<CreateObjectResult, Self::Error> {
        delegate!(self, |engine| engine.create_index(
            store_name,
            index_name,
            key_path,
            unique,
            multi_entry
        ))
    }

    fn delete_index(&self, store_name: &str, index_name: String) -> Result<(), Self::Error> {
        delegate!(self, |engine| engine.delete_index(store_name, index_name))
    }

    fn version(&self) -> Result<u64, Self::Error> {
        delegate!(self, |engine| engine.version())
    }

    fn set_version(&self, version: u64) -> Result<(), Self::Error> {
        delegate!(self, |engine| engine.set_version(version))
    }
}

#[cfg(test)]
mod tests {
    use servo_config::prefs::{self, Preferences};
    use servo_url::ImmutableOrigin;
    use tempfile::TempDir;
    use url::Host;

    use super::*;

    fn engine_with_pref(rdb_enabled: bool) -> (TempDir, TwinEngine) {
        prefs::set(Preferences {
            storage_ohos_rdb_backend_enabled: rdb_enabled,
            ..Preferences::default()
        });
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let description = IndexedDBDescription {
            name: "twin".to_owned(),
            origin: ImmutableOrigin::Tuple(
                "test_origin".to_owned(),
                Host::Domain("localhost".to_owned()),
                80,
            ),
        };
        let engine = TwinEngine::new(
            dir.path().to_path_buf(),
            true,
            &description,
            ThreadPool::global(),
        )
        .expect("Opening a database should not fail");
        (dir, engine)
    }

    /// The A/B switch: with both backends compiled in, the pref decides which
    /// one a database is opened on.
    #[test]
    fn pref_selects_the_backend() {
        assert!(matches!(engine_with_pref(true).1, TwinEngine::Rdb(_)));
        assert!(matches!(engine_with_pref(false).1, TwinEngine::Sqlite(_)));
    }
}
