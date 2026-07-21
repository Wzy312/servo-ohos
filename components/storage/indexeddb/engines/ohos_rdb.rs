/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(ohos_rdb)]

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use log::{info, warn};
use malloc_size_of_derive::MallocSizeOf;
use profile_traits::generic_callback::GenericCallback;
use serde::{Serialize, de::DeserializeOwned};
use servo_base::threadpool::ThreadPool;
use storage_traits::indexeddb::{
    AsyncOperation, AsyncReadOnlyOperation, AsyncReadWriteOperation, BackendError, BackendResult,
    CreateObjectResult, IndexedDBIndex, IndexedDBKeyRange, IndexedDBKeyType, IndexedDBRecord,
    IndexedDBTxnMode, KeyPath, PutItemResult,
};

use crate::indexeddb::IndexedDBDescription;
use crate::indexeddb::engines::encoding;
use crate::indexeddb::engines::shared::{ObjectDataModel, ObjectStoreModel};
use crate::indexeddb::engines::{KvsEngine, KvsTransaction, shared};
use crate::ohos_rdb::{
    OhosRdbCursor, OhosRdbError, OhosRdbStore, OhosRdbTransaction, OhosRdbValues,
    Result as OhosRdbResult,
};
use crate::shared::{DB_INIT_PRAGMAS, DB_PRAGMAS};

const INDEXEDDB_FILE_NAME: &str = "indexeddb.rdb";

type CommitSuccessAction = Box<dyn FnOnce() + Send>;
type CommitErrorAction = Box<dyn FnOnce(BackendError) + Send>;

#[derive(MallocSizeOf)]
pub(crate) struct OhosRdbEngine {
    #[ignore_malloc_size_of = "OHOS RDB handle"]
    store: Arc<Mutex<OhosRdbStore>>,
    #[ignore_malloc_size_of = "ThreadPool"]
    read_pool: Arc<ThreadPool>,
    #[ignore_malloc_size_of = "ThreadPool"]
    write_pool: Arc<ThreadPool>,
    created_db_path: bool,
}

impl OhosRdbEngine {
    pub(crate) fn new(
        path: PathBuf,
        created: bool,
        db_info: &IndexedDBDescription,
        pool: Arc<ThreadPool>,
    ) -> OhosRdbResult<Self> {
        fs::create_dir_all(&path)?;
        let store = OhosRdbStore::open(&path, INDEXEDDB_FILE_NAME)?;
        Self::init_db(&store, db_info)?;

        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            read_pool: pool.clone(),
            write_pool: pool,
            created_db_path: created,
        })
    }

    pub(crate) fn created_db_path(&self) -> bool {
        self.created_db_path
    }

    fn init_db(store: &OhosRdbStore, db_info: &IndexedDBDescription) -> OhosRdbResult<()> {
        // Match the SQLite backend's startup pragmas as closely as possible.
        for stmt in DB_INIT_PRAGMAS {
            let _ = store.execute(stmt);
        }

        Self::with_transaction(store, |tx| shared::init_db(tx, db_info))?;

        for stmt in DB_PRAGMAS {
            let _ = store.execute(stmt);
        }

        info!(
            "Initialized indexeddb database at {:?}",
            INDEXEDDB_FILE_NAME
        );
        Ok(())
    }

    fn with_transaction<R>(
        store: &OhosRdbStore,
        f: impl FnOnce(&OhosRdbTransaction) -> OhosRdbResult<R>,
    ) -> OhosRdbResult<R> {
        let tx = store.transaction()?;
        match f(&tx) {
            Ok(value) => {
                tx.commit()?;
                Ok(value)
            },
            Err(error) => {
                if let Err(rollback_error) = tx.rollback() {
                    warn!("Failed to roll back IndexedDB transaction: {rollback_error:?}");
                }
                Err(error)
            },
        }
    }

    fn with_store<R>(&self, f: impl FnOnce(&OhosRdbStore) -> OhosRdbResult<R>) -> OhosRdbResult<R> {
        let store = self
            .store
            .lock()
            .map_err(|_| Self::missing_row("OHOS RDB store lock"))?;
        f(&store)
    }

    fn missing_row(context: &'static str) -> OhosRdbError {
        OhosRdbError::Api { context, code: -1 }
    }

    fn object_store_by_name(
        tx: &OhosRdbTransaction,
        store_name: &str,
    ) -> OhosRdbResult<Option<ObjectStoreModel>> {
        let mut args = OhosRdbValues::new()?;
        args.push_text(store_name)?;
        let mut cursor = tx.query_sql(
            "SELECT id, key_path, auto_increment FROM object_store WHERE name = ?1;",
            &args,
        )?;
        if !cursor.next_row()? {
            return Ok(None);
        }

        Ok(Some(Self::read_object_store(&cursor, store_name)?))
    }

    fn read_object_store(
        cursor: &OhosRdbCursor<'_>,
        store_name: &str,
    ) -> OhosRdbResult<ObjectStoreModel> {
        // Every column here is NOT NULL in the schema; a NULL means a corrupt
        // row or a write from an incompatible version, so surface it instead
        // of masking it with a default (the SQLite twin errors the same way).
        Ok(ObjectStoreModel {
            id: cursor
                .int64(0)?
                .ok_or_else(|| Self::missing_row("object_store.id"))?,
            name: store_name.to_owned(),
            key_path: cursor.blob(1)?,
            auto_increment: cursor
                .int64(2)?
                .ok_or_else(|| Self::missing_row("object_store.auto_increment"))?,
        })
    }

    fn read_object_data(cursor: &OhosRdbCursor<'_>) -> OhosRdbResult<ObjectDataModel> {
        Ok(ObjectDataModel {
            object_store_id: cursor
                .int64(0)?
                .ok_or_else(|| Self::missing_row("object_data.object_store_id"))?,
            key: cursor
                .blob(1)?
                .ok_or_else(|| Self::missing_row("object_data.key"))?,
            data: cursor
                .blob(2)?
                .ok_or_else(|| Self::missing_row("object_data.data"))?,
        })
    }

    fn decode_key(data: &[u8], context: &'static str) -> OhosRdbResult<IndexedDBKeyType> {
        encoding::deserialize(data).ok_or_else(|| Self::missing_row(context))
    }

    fn enqueue_result<T>(
        success_actions: &mut Vec<CommitSuccessAction>,
        commit_error_actions: &mut Vec<CommitErrorAction>,
        callback: GenericCallback<BackendResult<T>>,
        result: BackendResult<T>,
    ) where
        T: Clone + Serialize + DeserializeOwned + Send + 'static,
    {
        let commit_error_callback = callback.clone();
        success_actions.push(Box::new(move || {
            let _ = callback.send(result);
        }));
        commit_error_actions.push(Box::new(move |error: BackendError| {
            let _ = commit_error_callback.send(Err(error));
        }));
    }

    fn enqueue_operation_error(
        success_actions: &mut Vec<CommitSuccessAction>,
        commit_error_actions: &mut Vec<CommitErrorAction>,
        operation: AsyncOperation,
        error: BackendError,
    ) {
        match operation {
            AsyncOperation::ReadOnly(operation) => match operation {
                AsyncReadOnlyOperation::GetKey { callback, .. } => Self::enqueue_result(
                    success_actions,
                    commit_error_actions,
                    callback,
                    Err(error),
                ),
                AsyncReadOnlyOperation::GetItem { callback, .. } => Self::enqueue_result(
                    success_actions,
                    commit_error_actions,
                    callback,
                    Err(error),
                ),
                AsyncReadOnlyOperation::GetAllKeys { callback, .. } => Self::enqueue_result(
                    success_actions,
                    commit_error_actions,
                    callback,
                    Err(error),
                ),
                AsyncReadOnlyOperation::GetAllItems { callback, .. } => Self::enqueue_result(
                    success_actions,
                    commit_error_actions,
                    callback,
                    Err(error),
                ),
                AsyncReadOnlyOperation::Count { callback, .. } => Self::enqueue_result(
                    success_actions,
                    commit_error_actions,
                    callback,
                    Err(error),
                ),
                AsyncReadOnlyOperation::Iterate { callback, .. } => Self::enqueue_result(
                    success_actions,
                    commit_error_actions,
                    callback,
                    Err(error),
                ),
            },
            AsyncOperation::ReadWrite(operation) => match operation {
                AsyncReadWriteOperation::PutItem { callback, .. } => Self::enqueue_result(
                    success_actions,
                    commit_error_actions,
                    callback,
                    Err(error),
                ),
                AsyncReadWriteOperation::RemoveItem { callback, .. } => Self::enqueue_result(
                    success_actions,
                    commit_error_actions,
                    callback,
                    Err(error),
                ),
                AsyncReadWriteOperation::Clear(callback) => Self::enqueue_result(
                    success_actions,
                    commit_error_actions,
                    callback,
                    Err(error),
                ),
            },
        }
    }

    fn query_optional_object_store(
        tx: &OhosRdbTransaction,
        store_name: &str,
    ) -> OhosRdbResult<Option<ObjectStoreModel>> {
        Self::object_store_by_name(tx, store_name)
    }

    fn object_data_rows(
        tx: &OhosRdbTransaction,
        store: &ObjectStoreModel,
        key_range: IndexedDBKeyRange,
        count: Option<u32>,
    ) -> OhosRdbResult<Vec<ObjectDataModel>> {
        let (sql, args) = shared::object_data_select_sql::<OhosRdbTransaction>(
            "object_store_id, key, data",
            store.id,
            key_range,
            count,
            true,
        )?;
        let mut cursor = tx.query_sql(&sql, &args)?;
        let mut rows = Vec::new();
        while cursor.next_row()? {
            rows.push(Self::read_object_data(&cursor)?);
        }
        Ok(rows)
    }

    fn put_item(
        tx: &OhosRdbTransaction,
        store: &ObjectStoreModel,
        key: IndexedDBKeyType,
        value: Vec<u8>,
        should_overwrite: bool,
        key_generator_current_number: Option<i64>,
    ) -> OhosRdbResult<PutItemResult> {
        shared::put_item(
            tx,
            store,
            key,
            value,
            should_overwrite,
            key_generator_current_number,
        )
    }

    fn delete_item(
        tx: &OhosRdbTransaction,
        store: &ObjectStoreModel,
        key_range: IndexedDBKeyRange,
    ) -> OhosRdbResult<()> {
        shared::delete_item(tx, store, key_range)
    }

    fn clear(tx: &OhosRdbTransaction, store: &ObjectStoreModel) -> OhosRdbResult<()> {
        shared::clear(tx, store)
    }

    fn count(
        tx: &OhosRdbTransaction,
        store: &ObjectStoreModel,
        key_range: IndexedDBKeyRange,
    ) -> OhosRdbResult<usize> {
        shared::count(tx, store, key_range)
    }
}

impl KvsEngine for OhosRdbEngine {
    type Error = OhosRdbError;

    fn create_store(
        &self,
        store_name: &str,
        key_path: Option<KeyPath>,
        auto_increment: bool,
    ) -> Result<CreateObjectResult, Self::Error> {
        self.with_store(|store| {
            Self::with_transaction(store, |tx| {
                shared::create_store(
                    tx,
                    store_name,
                    key_path,
                    auto_increment,
                    Self::missing_row,
                    |err| err.into(),
                )
            })
        })
    }

    fn delete_store(&self, store_name: &str) -> Result<(), Self::Error> {
        self.with_store(|store| {
            Self::with_transaction(store, |tx| {
                shared::delete_store(tx, store_name, |_| Self::missing_row("object store lookup"))
            })
        })
    }

    fn process_transaction(
        &self,
        transaction: KvsTransaction,
        on_complete: Box<dyn FnOnce() + Send + 'static>,
    ) {
        let spawning_pool = if transaction.mode == IndexedDBTxnMode::Readonly {
            self.read_pool.clone()
        } else {
            self.write_pool.clone()
        };
        let store = self.store.clone();
        spawning_pool.spawn(move || {
            // Callback-timing divergence from the SQLite twin, by design: the
            // SQLite engine runs each request on an autocommit connection and
            // reports it immediately, while this engine defers every callback
            // until commit() succeeds so no success is reported for a write
            // that never became durable; if the commit fails, the whole batch
            // reports Err (matching IndexedDB all-or-nothing transaction
            // semantics).
            let mut success_actions: Vec<CommitSuccessAction> = Vec::new();
            let mut commit_error_actions: Vec<CommitErrorAction> = Vec::new();

            // The guard's scope is deliberately just the transaction creation:
            // the Mutex serializes calls on the shared store handle, while the
            // batch below (including commit) drives the transaction's own
            // dedicated native connection, which the native library
            // synchronizes internally. See the Send justification on
            // `OhosRdbStore` for the evidence.
            let tx = {
                let creation = store
                    .lock()
                    .map_err(|error| format!("{error:?}"))
                    .and_then(|store| store.transaction().map_err(|error| format!("{error:?}")));
                match creation {
                    Ok(tx) => tx,
                    Err(error) => {
                        for request in transaction.requests {
                            Self::enqueue_operation_error(
                                &mut success_actions,
                                &mut commit_error_actions,
                                request.operation,
                                BackendError::DbErr(error.clone()),
                            );
                        }
                        for action in success_actions {
                            action();
                        }
                        on_complete();
                        return;
                    },
                }
            };

            for request in transaction.requests {
                let object_store = match Self::query_optional_object_store(&tx, &request.store_name)
                {
                    Ok(Some(store)) => store,
                    Ok(None) => {
                        Self::enqueue_operation_error(
                            &mut success_actions,
                            &mut commit_error_actions,
                            request.operation,
                            BackendError::StoreNotFound,
                        );
                        continue;
                    },
                    Err(error) => {
                        Self::enqueue_operation_error(
                            &mut success_actions,
                            &mut commit_error_actions,
                            request.operation,
                            BackendError::DbErr(format!("{error:?}")),
                        );
                        continue;
                    },
                };

                match request.operation {
                    AsyncOperation::ReadWrite(AsyncReadWriteOperation::PutItem {
                        callback,
                        key,
                        value,
                        should_overwrite,
                        key_generator_current_number,
                    }) => {
                        let (key, key_generator_current_number) = match key {
                            Some(key) => (key, key_generator_current_number),
                            None => {
                                if object_store.auto_increment == 0 {
                                    Self::enqueue_result(
                                        &mut success_actions,
                                        &mut commit_error_actions,
                                        callback,
                                        Err(BackendError::DbErr(
                                            "Missing key for PutItem request".to_string(),
                                        )),
                                    );
                                    continue;
                                }
                                let Some(next_key_generator_current_number) =
                                    object_store.auto_increment.checked_add(1)
                                else {
                                    Self::enqueue_result(
                                        &mut success_actions,
                                        &mut commit_error_actions,
                                        callback,
                                        Err(BackendError::DbErr(
                                            "Key generator overflow".to_string(),
                                        )),
                                    );
                                    continue;
                                };
                                (
                                    IndexedDBKeyType::Number(object_store.auto_increment as f64),
                                    Some(next_key_generator_current_number),
                                )
                            },
                        };
                        Self::enqueue_result(
                            &mut success_actions,
                            &mut commit_error_actions,
                            callback,
                            Self::put_item(
                                &tx,
                                &object_store,
                                key,
                                value,
                                should_overwrite,
                                key_generator_current_number,
                            )
                            .map_err(|error| BackendError::DbErr(format!("{error:?}"))),
                        );
                    },
                    AsyncOperation::ReadOnly(AsyncReadOnlyOperation::GetItem {
                        callback,
                        key_range,
                    }) => {
                        Self::enqueue_result(
                            &mut success_actions,
                            &mut commit_error_actions,
                            callback,
                            Self::object_data_rows(&tx, &object_store, key_range, Some(1))
                                .map(|mut rows| rows.pop().map(|row| row.data))
                                .map_err(|error| BackendError::DbErr(format!("{error:?}"))),
                        );
                    },
                    AsyncOperation::ReadOnly(AsyncReadOnlyOperation::GetAllKeys {
                        callback,
                        key_range,
                        count,
                    }) => {
                        let result = Self::object_data_rows(&tx, &object_store, key_range, count)
                            .and_then(|rows| {
                                rows.into_iter()
                                    .map(|row| Self::decode_key(&row.key, "object data key decode"))
                                    .collect::<OhosRdbResult<Vec<_>>>()
                            })
                            .map_err(|error| BackendError::DbErr(format!("{error:?}")));
                        Self::enqueue_result(
                            &mut success_actions,
                            &mut commit_error_actions,
                            callback,
                            result,
                        );
                    },
                    AsyncOperation::ReadOnly(AsyncReadOnlyOperation::GetAllItems {
                        callback,
                        key_range,
                        count,
                    }) => {
                        Self::enqueue_result(
                            &mut success_actions,
                            &mut commit_error_actions,
                            callback,
                            Self::object_data_rows(&tx, &object_store, key_range, count)
                                .map(|rows| rows.into_iter().map(|row| row.data).collect())
                                .map_err(|error| BackendError::DbErr(format!("{error:?}"))),
                        );
                    },
                    AsyncOperation::ReadWrite(AsyncReadWriteOperation::RemoveItem {
                        callback,
                        key_range,
                    }) => {
                        Self::enqueue_result(
                            &mut success_actions,
                            &mut commit_error_actions,
                            callback,
                            Self::delete_item(&tx, &object_store, key_range)
                                .map_err(|error| BackendError::DbErr(format!("{error:?}"))),
                        );
                    },
                    AsyncOperation::ReadOnly(AsyncReadOnlyOperation::Count {
                        callback,
                        key_range,
                    }) => {
                        Self::enqueue_result(
                            &mut success_actions,
                            &mut commit_error_actions,
                            callback,
                            Self::count(&tx, &object_store, key_range)
                                .map(|count| count as u64)
                                .map_err(|error| BackendError::DbErr(format!("{error:?}"))),
                        );
                    },
                    AsyncOperation::ReadOnly(AsyncReadOnlyOperation::Iterate {
                        callback,
                        key_range,
                    }) => {
                        let result = Self::object_data_rows(&tx, &object_store, key_range, None)
                            .and_then(|rows| {
                                rows.into_iter()
                                    .map(|row| {
                                        let key =
                                            Self::decode_key(&row.key, "object data key decode")?;
                                        Ok(IndexedDBRecord {
                                            key: key.clone(),
                                            primary_key: key,
                                            value: row.data,
                                        })
                                    })
                                    .collect::<OhosRdbResult<Vec<_>>>()
                            })
                            .map_err(|error| BackendError::DbErr(format!("{error:?}")));
                        Self::enqueue_result(
                            &mut success_actions,
                            &mut commit_error_actions,
                            callback,
                            result,
                        );
                    },
                    AsyncOperation::ReadWrite(AsyncReadWriteOperation::Clear(sender)) => {
                        Self::enqueue_result(
                            &mut success_actions,
                            &mut commit_error_actions,
                            sender,
                            Self::clear(&tx, &object_store)
                                .map_err(|error| BackendError::DbErr(format!("{error:?}"))),
                        );
                    },
                    AsyncOperation::ReadOnly(AsyncReadOnlyOperation::GetKey {
                        callback,
                        key_range,
                    }) => {
                        let result = Self::object_data_rows(&tx, &object_store, key_range, Some(1))
                            .and_then(|mut rows| {
                                rows.pop()
                                    .map(|row| Self::decode_key(&row.key, "object data key decode"))
                                    .transpose()
                            })
                            .map_err(|error| BackendError::DbErr(format!("{error:?}")));
                        Self::enqueue_result(
                            &mut success_actions,
                            &mut commit_error_actions,
                            callback,
                            result,
                        );
                    },
                }
            }

            match tx.commit() {
                Ok(()) => {
                    for action in success_actions {
                        action();
                    }
                },
                Err(error) => {
                    warn!("Failed to commit IndexedDB transaction: {error:?}");
                    let commit_error = BackendError::DbErr(format!("{error:?}"));
                    for action in commit_error_actions {
                        action(commit_error.clone());
                    }
                },
            }

            on_complete();
        });
    }

    fn key_generator_current_number(&self, store_name: &str) -> Option<i64> {
        self.store.lock().ok().and_then(|store| {
            let tx = store.transaction().ok()?;
            let result =
                shared::key_generator_current_number(&tx, store_name, Self::missing_row).ok()?;
            if let Err(error) = tx.commit() {
                warn!("Failed to commit IndexedDB key_generator_current_number lookup: {error:?}");
                return None;
            }
            result
        })
    }

    fn set_key_generator_current_number(
        &self,
        store_name: &str,
        current_number: i64,
    ) -> Result<(), Self::Error> {
        self.with_store(|store| {
            Self::with_transaction(store, |tx| {
                shared::set_key_generator_current_number(tx, store_name, current_number, |_| {
                    Self::missing_row("object store lookup")
                })
            })
        })
    }

    fn key_path(&self, store_name: &str) -> Option<KeyPath> {
        let store = match self.store.lock() {
            Ok(store) => store,
            Err(error) => {
                warn!("IndexedDB key_path lookup failed to lock the store: {error:?}");
                return None;
            },
        };
        let tx = match store.transaction() {
            Ok(tx) => tx,
            Err(error) => {
                warn!("IndexedDB key_path lookup failed to open a transaction: {error:?}");
                return None;
            },
        };
        let result = match shared::key_path(&tx, store_name, Self::missing_row, |err| err.into()) {
            Ok(result) => result,
            Err(error) => {
                warn!("IndexedDB key_path lookup failed: {error:?}");
                return None;
            },
        };
        if let Err(error) = tx.commit() {
            warn!("Failed to commit IndexedDB key_path lookup: {error:?}");
            return None;
        }
        result
    }

    fn close_store(&self, _store_name: &str) -> Result<(), Self::Error> {
        Ok(())
    }

    fn object_store_names(&self) -> Result<Vec<String>, Self::Error> {
        self.with_store(|store| Self::with_transaction(store, shared::object_store_names))
    }

    fn indexes(&self, store_name: &str) -> Result<Vec<IndexedDBIndex>, Self::Error> {
        self.with_store(|store| {
            Self::with_transaction(store, |tx| {
                shared::indexes(
                    tx,
                    store_name,
                    |_| Self::missing_row("object store lookup"),
                    |err| err.into(),
                )
            })
        })
    }

    fn create_index(
        &self,
        store_name: &str,
        index_name: String,
        key_path: KeyPath,
        unique: bool,
        multi_entry: bool,
    ) -> Result<CreateObjectResult, Self::Error> {
        self.with_store(|store| {
            Self::with_transaction(store, |tx| {
                shared::create_index(
                    tx,
                    store_name,
                    index_name,
                    key_path,
                    unique,
                    multi_entry,
                    |_| Self::missing_row("object store lookup"),
                    |err| err.into(),
                )
            })
        })
    }

    fn delete_index(&self, store_name: &str, index_name: String) -> Result<(), Self::Error> {
        self.with_store(|store| {
            Self::with_transaction(store, |tx| {
                shared::delete_index(tx, store_name, index_name, |_| {
                    Self::missing_row("object store lookup")
                })
            })
        })
    }

    fn version(&self) -> Result<u64, Self::Error> {
        self.with_store(|store| {
            Self::with_transaction(store, |tx| {
                shared::version(tx, |_| Self::missing_row("database version lookup"))
            })
        })
    }

    fn set_version(&self, version: u64) -> Result<(), Self::Error> {
        self.with_store(|store| {
            Self::with_transaction(store, |tx| {
                shared::set_version(tx, version, |_| {
                    Self::missing_row("database version update")
                })
            })
        })
    }
}
