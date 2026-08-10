/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::collections::VecDeque;

use malloc_size_of::MallocSizeOf;
use malloc_size_of_derive::MallocSizeOf;
use storage_traits::indexeddb::{
    AsyncOperation, CreateObjectResult, IndexedDBIndex, IndexedDBTxnMode, KeyPath,
};

#[cfg(ohos_rdb)]
mod ohos_rdb;

#[path = "sqlite/encoding.rs"]
pub(crate) mod encoding;

#[cfg(ohos_rdb)]
pub(crate) mod shared;

#[cfg(feature = "sqlite-backend")]
mod sqlite;

#[cfg(all(feature = "sqlite-backend", ohos_rdb))]
mod twin;

// `ActiveKvsEngine` is the engine the IndexedDB manager runs on. A build with
// both backends compiled in resolves it to the twin, which picks one per
// database open, so call sites stay untouched in every configuration.
#[cfg(all(not(feature = "sqlite-backend"), ohos_rdb))]
pub(crate) use ohos_rdb::OhosRdbEngine as ActiveKvsEngine;
#[cfg(all(feature = "sqlite-backend", ohos_rdb))]
pub(crate) use twin::TwinEngine as ActiveKvsEngine;

#[cfg(all(feature = "sqlite-backend", not(ohos_rdb)))]
pub(crate) use self::sqlite::SqliteEngine as ActiveKvsEngine;
#[cfg(feature = "sqlite-backend")]
#[allow(unused_imports)]
pub(crate) use self::sqlite::SqliteEngine;

#[derive(MallocSizeOf)]
pub struct KvsOperation {
    pub store_name: String,
    pub operation: AsyncOperation,
}

#[derive(MallocSizeOf)]
pub struct KvsTransaction {
    // Mode could be used by a more optimal implementation of transactions
    // that has different allocated threadpools for reading and writing
    pub mode: IndexedDBTxnMode,
    pub requests: VecDeque<KvsOperation>,
}

pub trait KvsEngine: MallocSizeOf {
    type Error: std::error::Error;

    fn create_store(
        &self,
        store_name: &str,
        key_path: Option<KeyPath>,
        auto_increment: bool,
    ) -> Result<CreateObjectResult, Self::Error>;

    fn delete_store(&self, store_name: &str) -> Result<(), Self::Error>;

    // Unused with a single backend compiled in; the twin engine delegates it.
    #[allow(dead_code)]
    fn close_store(&self, store_name: &str) -> Result<(), Self::Error>;

    fn process_transaction(
        &self,
        transaction: KvsTransaction,
        on_complete: Box<dyn FnOnce() + Send + 'static>,
    );

    fn key_generator_current_number(&self, store_name: &str) -> Option<i64>;
    fn set_key_generator_current_number(
        &self,
        store_name: &str,
        current_number: i64,
    ) -> Result<(), Self::Error>;
    fn key_path(&self, store_name: &str) -> Option<KeyPath>;
    fn object_store_names(&self) -> Result<Vec<String>, Self::Error>;
    fn indexes(&self, store_name: &str) -> Result<Vec<IndexedDBIndex>, Self::Error>;

    fn create_index(
        &self,
        store_name: &str,
        index_name: String,
        key_path: KeyPath,
        unique: bool,
        multi_entry: bool,
    ) -> Result<CreateObjectResult, Self::Error>;
    fn delete_index(&self, store_name: &str, index_name: String) -> Result<(), Self::Error>;

    fn version(&self) -> Result<u64, Self::Error>;
    fn set_version(&self, version: u64) -> Result<(), Self::Error>;
}
