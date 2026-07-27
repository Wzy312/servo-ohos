/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::Cell;
use std::sync::{Mutex, MutexGuard, OnceLock, mpsc};
use std::time::Duration;

use profile::mem as profile_mem;
use profile_traits::generic_callback::GenericCallback as ProfileGenericCallback;
use profile_traits::time::ProfilerChan as TimeProfilerChan;
use servo_base::generic_channel::{self, GenericSend};
use servo_base::id::{
    PIPELINE_NAMESPACE, PipelineNamespace, PipelineNamespaceId, TEST_PAINTER_ID, WebViewId,
};
use servo_url::{ImmutableOrigin, ServoUrl};
use storage::ClientStorageThreadFactory;
use storage_traits::StorageThreads;
use storage_traits::client_storage::{StorageIdentifier, StorageProxyMap, StorageType};
use storage_traits::indexeddb::{
    AsyncOperation, AsyncReadOnlyOperation, AsyncReadWriteOperation, BackendResult, ConnectionMsg,
    CreateObjectResult, IndexedDBKeyRange, IndexedDBKeyType, IndexedDBObjectStore,
    IndexedDBThreadMsg, IndexedDBTxnMode, KeyPath, PutItemResult, SyncOperation, TxnCompleteMsg,
};
use tempfile::TempDir;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(5);

static INDEXEDDB_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn indexeddb_test_lock() -> &'static Mutex<()> {
    INDEXEDDB_TEST_LOCK.get_or_init(|| Mutex::new(()))
}

fn install_test_namespace() {
    if PIPELINE_NAMESPACE.get().is_none() {
        PipelineNamespace::install(PipelineNamespaceId(1));
    }
}

fn shutdown_storage_group(threads: &StorageThreads) {
    let (client_sender, client_receiver) = generic_channel::channel().unwrap();
    GenericSend::send(
        threads,
        storage_traits::client_storage::ClientStorageThreadMessage::Exit(client_sender),
    )
    .expect("failed to send client storage exit");
    client_receiver
        .recv()
        .expect("failed to receive client storage exit ack");

    let (idb_sender, idb_receiver) = generic_channel::channel().unwrap();
    GenericSend::send(
        threads,
        IndexedDBThreadMsg::Sync(SyncOperation::Exit(idb_sender)),
    )
    .expect("failed to send indexeddb exit");
    idb_receiver
        .recv()
        .expect("failed to receive indexeddb exit ack");

    let (web_storage_sender, web_storage_receiver) = generic_channel::channel().unwrap();
    GenericSend::send(
        threads,
        storage_traits::webstorage_thread::WebStorageThreadMsg::Exit(web_storage_sender),
    )
    .expect("failed to send web storage exit");
    web_storage_receiver
        .recv()
        .expect("failed to receive web storage exit ack");
}

struct IndexedDbTestContext {
    _client_handle: storage_traits::client_storage::ClientStorageThreadHandle,
    _test_lock: MutexGuard<'static, ()>,
    _temp_dir: TempDir,
    private_threads: StorageThreads,
    public_threads: StorageThreads,
    proxy_map: StorageProxyMap,
    origin: ImmutableOrigin,
    shutdown: Cell<bool>,
}

impl IndexedDbTestContext {
    fn new() -> Self {
        install_test_namespace();
        let test_lock = indexeddb_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let config_dir = temp_dir.path().to_path_buf();
        let client_handle: storage_traits::client_storage::ClientStorageThreadHandle =
            ClientStorageThreadFactory::new(Some(config_dir.clone()), false);
        let mem_profiler_chan = profile_mem::Profiler::create();
        let (private_threads, public_threads) =
            storage::new_storage_threads(mem_profiler_chan, Some(config_dir), false);
        let origin = ServoUrl::parse("https://example.com").unwrap().origin();
        let proxy_map = client_handle
            .obtain_a_storage_bottle_map(
                StorageType::Local,
                Some(WebViewId::new(TEST_PAINTER_ID)),
                StorageIdentifier::IndexedDB,
                origin.clone(),
            )
            .recv()
            .unwrap()
            .unwrap();

        Self {
            _client_handle: client_handle,
            _test_lock: test_lock,
            _temp_dir: temp_dir,
            private_threads,
            public_threads,
            proxy_map,
            origin,
            shutdown: Cell::new(false),
        }
    }

    fn shutdown(&self) {
        if self.shutdown.replace(true) {
            return;
        }

        shutdown_storage_group(&self.private_threads);
        shutdown_storage_group(&self.public_threads);
    }
}

impl Drop for IndexedDbTestContext {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn recv_timeout<T>(receiver: &generic_channel::GenericReceiver<T>, what: &str) -> T
where
    T: for<'de> serde::Deserialize<'de> + serde::Serialize,
{
    receiver
        .try_recv_timeout(TIMEOUT)
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

fn make_profile_callback<T>() -> (ProfileGenericCallback<T>, mpsc::Receiver<Option<T>>)
where
    T: for<'de> serde::Deserialize<'de> + serde::Serialize + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let callback = ProfileGenericCallback::new(TimeProfilerChan(None), move |result| {
        let _ = sender.send(result.ok());
    })
    .unwrap();
    (callback, receiver)
}

fn recv_callback_timeout<T>(receiver: &mpsc::Receiver<Option<T>>, what: &str) -> T {
    receiver
        .recv_timeout(TIMEOUT)
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
        .unwrap_or_else(|| panic!("{what} callback failed"))
}

fn open_database(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    version: Option<u64>,
) -> mpsc::Receiver<Option<ConnectionMsg>> {
    let (sender, receiver) = make_profile_callback();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::OpenDatabase(
            sender,
            ctx.origin.clone(),
            db_name.to_string(),
            version,
            Uuid::new_v4(),
            ctx.proxy_map.clone(),
        )),
    )
    .unwrap();
    receiver
}

fn create_object_store(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
    key_path: Option<KeyPath>,
    auto_increment: bool,
) -> CreateObjectResult {
    let (sender, receiver) = generic_channel::channel().unwrap();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::CreateObjectStore(
            sender,
            ctx.origin.clone(),
            db_name.to_string(),
            store_name.to_string(),
            key_path,
            auto_increment,
        )),
    )
    .unwrap();
    recv_timeout(&receiver, "create object store reply").unwrap()
}

fn create_index(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
    index_name: &str,
    key_path: KeyPath,
    unique: bool,
    multi_entry: bool,
) {
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::CreateIndex(
            ctx.origin.clone(),
            db_name.to_string(),
            store_name.to_string(),
            index_name.to_string(),
            key_path,
            unique,
            multi_entry,
        )),
    )
    .unwrap();
}

fn get_object_store(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
) -> IndexedDBObjectStore {
    let (sender, receiver) = generic_channel::channel().unwrap();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::GetObjectStore(
            sender,
            ctx.origin.clone(),
            db_name.to_string(),
            store_name.to_string(),
        )),
    )
    .unwrap();
    recv_timeout(&receiver, "get object store reply").unwrap()
}

fn get_version(ctx: &IndexedDbTestContext, db_name: &str) -> u64 {
    let (sender, receiver) = generic_channel::channel().unwrap();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::Version(
            sender,
            ctx.origin.clone(),
            db_name.to_string(),
        )),
    )
    .unwrap();
    recv_timeout(&receiver, "version reply").unwrap()
}

fn create_transaction(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    mode: IndexedDBTxnMode,
    scope: Vec<String>,
) -> u64 {
    let (sender, receiver) = generic_channel::channel().unwrap();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::CreateTransaction {
            sender,
            origin: ctx.origin.clone(),
            db_name: db_name.to_string(),
            mode,
            scope,
        }),
    )
    .unwrap();
    recv_timeout(&receiver, "create transaction reply").unwrap()
}

fn commit_transaction(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    txn: u64,
) -> mpsc::Receiver<Option<TxnCompleteMsg>> {
    let (sender, receiver) = make_profile_callback();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::Commit(
            sender,
            ctx.origin.clone(),
            db_name.to_string(),
            txn,
        )),
    )
    .unwrap();
    receiver
}

fn close_database(ctx: &IndexedDbTestContext, id: Uuid, db_name: &str) {
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::CloseDatabase(
            ctx.origin.clone(),
            id,
            db_name.to_string(),
        )),
    )
    .unwrap();
}

fn finish_upgrade_transaction(
    ctx: &IndexedDbTestContext,
    txn: u64,
    db_name: &str,
    committed: bool,
) {
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::UpgradeTransactionFinished {
            txn,
            db_name: db_name.to_string(),
            origin: ctx.origin.clone(),
            committed,
        }),
    )
    .unwrap();
}

fn mark_request_handled(ctx: &IndexedDbTestContext, db_name: &str, txn: u64, request_id: u64) {
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::RequestHandled {
            origin: ctx.origin.clone(),
            db_name: db_name.to_string(),
            txn,
            request_id,
        }),
    )
    .unwrap();
}

fn finish_transaction(ctx: &IndexedDbTestContext, db_name: &str, txn: u64) {
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::TransactionFinished {
            origin: ctx.origin.clone(),
            db_name: db_name.to_string(),
            txn,
        }),
    )
    .unwrap();
}

fn put_item(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
    txn: u64,
    key: IndexedDBKeyType,
    value: Vec<u8>,
) -> mpsc::Receiver<Option<BackendResult<PutItemResult>>> {
    put_item_request(ctx, db_name, store_name, txn, 1, Some(key), value, true)
}

fn put_item_request(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
    txn: u64,
    request_id: u64,
    key: Option<IndexedDBKeyType>,
    value: Vec<u8>,
    should_overwrite: bool,
) -> mpsc::Receiver<Option<BackendResult<PutItemResult>>> {
    let (sender, receiver) = make_profile_callback();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Async(
            ctx.origin.clone(),
            db_name.to_string(),
            store_name.to_string(),
            txn,
            request_id,
            IndexedDBTxnMode::Readwrite,
            AsyncOperation::ReadWrite(AsyncReadWriteOperation::PutItem {
                callback: sender,
                key,
                value,
                should_overwrite,
                key_generator_current_number: None,
            }),
        ),
    )
    .unwrap();
    receiver
}

fn get_all_keys_request(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
    txn: u64,
    key_range: IndexedDBKeyRange,
    request_id: u64,
) -> mpsc::Receiver<Option<BackendResult<Vec<IndexedDBKeyType>>>> {
    let (sender, receiver) = make_profile_callback();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Async(
            ctx.origin.clone(),
            db_name.to_string(),
            store_name.to_string(),
            txn,
            request_id,
            IndexedDBTxnMode::Readonly,
            AsyncOperation::ReadOnly(AsyncReadOnlyOperation::GetAllKeys {
                callback: sender,
                key_range,
                count: None,
            }),
        ),
    )
    .unwrap();
    receiver
}

fn get_item(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
    txn: u64,
    key: IndexedDBKeyType,
) -> mpsc::Receiver<Option<BackendResult<Option<Vec<u8>>>>> {
    let request_id = 1;
    let (sender, receiver) = make_profile_callback();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Async(
            ctx.origin.clone(),
            db_name.to_string(),
            store_name.to_string(),
            txn,
            request_id,
            IndexedDBTxnMode::Readonly,
            AsyncOperation::ReadOnly(AsyncReadOnlyOperation::GetItem {
                callback: sender,
                key_range: IndexedDBKeyRange::only(key),
            }),
        ),
    )
    .unwrap();
    receiver
}

fn remove_item(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
    txn: u64,
    key: IndexedDBKeyType,
) -> mpsc::Receiver<Option<BackendResult<()>>> {
    let request_id = 1;
    let (sender, receiver) = make_profile_callback();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Async(
            ctx.origin.clone(),
            db_name.to_string(),
            store_name.to_string(),
            txn,
            request_id,
            IndexedDBTxnMode::Readwrite,
            AsyncOperation::ReadWrite(AsyncReadWriteOperation::RemoveItem {
                callback: sender,
                key_range: IndexedDBKeyRange::only(key),
            }),
        ),
    )
    .unwrap();
    receiver
}

fn remove_item_request(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
    txn: u64,
    key_range: IndexedDBKeyRange,
) -> mpsc::Receiver<Option<BackendResult<()>>> {
    let request_id = 1;
    let (sender, receiver) = make_profile_callback();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Async(
            ctx.origin.clone(),
            db_name.to_string(),
            store_name.to_string(),
            txn,
            request_id,
            IndexedDBTxnMode::Readwrite,
            AsyncOperation::ReadWrite(AsyncReadWriteOperation::RemoveItem {
                callback: sender,
                key_range,
            }),
        ),
    )
    .unwrap();
    receiver
}

fn expect_upgrade_and_initialize_database(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    store_name: &str,
    index_name: &str,
    store_key_path: Option<KeyPath>,
) -> Uuid {
    let receiver = open_database(ctx, db_name, Some(1));

    loop {
        match recv_callback_timeout(&receiver, "open database reply") {
            ConnectionMsg::Upgrade {
                id: _,
                name,
                version,
                old_version,
                transaction,
                object_store_names,
            } => {
                assert_eq!(name, db_name);
                assert_eq!(version, 1);
                assert_eq!(old_version, 0);
                assert!(object_store_names.is_empty());

                assert_eq!(
                    create_object_store(ctx, db_name, store_name, store_key_path.clone(), false),
                    CreateObjectResult::Created
                );
                create_index(
                    ctx,
                    db_name,
                    store_name,
                    index_name,
                    KeyPath::String("author".to_string()),
                    false,
                    false,
                );
                // Script sends both signals for a versionchange transaction
                // (idbtransaction.rs dispatch_complete): the upgrade-specific
                // one first, then the generic finished one that releases the
                // transaction's scope in the scheduler.
                finish_upgrade_transaction(ctx, transaction, db_name, true);
                finish_transaction(ctx, db_name, transaction);
            },
            ConnectionMsg::Connection {
                id,
                name,
                version,
                upgraded,
                object_store_names,
            } => {
                assert_eq!(name, db_name);
                assert_eq!(version, 1);
                assert!(upgraded);
                assert_eq!(object_store_names, vec![store_name.to_string()]);
                return id;
            },
            other => panic!("unexpected connection message: {other:?}"),
        }
    }
}

fn reopen_database(ctx: &IndexedDbTestContext, db_name: &str, store_name: &str) -> Uuid {
    let receiver = open_database(ctx, db_name, None);
    loop {
        match recv_callback_timeout(&receiver, "reopen database reply") {
            ConnectionMsg::Connection {
                id,
                name,
                version,
                upgraded,
                object_store_names,
            } => {
                assert_eq!(name, db_name);
                assert_eq!(version, 1);
                assert!(!upgraded);
                assert_eq!(object_store_names, vec![store_name.to_string()]);
                return id;
            },
            other => panic!("unexpected reopen message: {other:?}"),
        }
    }
}

#[test]
fn test_indexeddb_roundtrip_and_metadata_persist() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-roundtrip";
    let store_name = "books";
    let index_name = "by_author";
    let connection_id = expect_upgrade_and_initialize_database(
        &ctx,
        db_name,
        store_name,
        index_name,
        Some(KeyPath::String("id".to_string())),
    );

    assert_eq!(get_version(&ctx, db_name), 1);
    let store = get_object_store(&ctx, db_name, store_name);
    assert_eq!(store.name, store_name);
    assert_eq!(store.key_path, Some(KeyPath::String("id".to_string())));
    assert_eq!(store.has_key_generator, false);
    assert_eq!(store.key_generator_current_number, None);
    assert_eq!(store.indexes.len(), 1);
    assert_eq!(store.indexes[0].name, index_name);
    assert_eq!(
        store.indexes[0].key_path,
        KeyPath::String("author".to_string())
    );
    assert!(!store.indexes[0].multi_entry);
    assert!(!store.indexes[0].unique);
    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readwrite,
        vec![store_name.to_string()],
    );
    let put_result = put_item(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyType::String("book-1".to_string()),
        b"Moby Dick".to_vec(),
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let put_msg = recv_callback_timeout(&put_result, "put item reply").unwrap();
    mark_request_handled(&ctx, db_name, txn, 1);
    let commit_msg = recv_callback_timeout(&commit_result, "commit reply");
    assert_eq!(
        put_msg,
        PutItemResult::Key(IndexedDBKeyType::String("book-1".to_string()))
    );
    assert_eq!(commit_msg.txn, txn);
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readonly,
        vec![store_name.to_string()],
    );
    let roundtrip = get_item(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyType::String("book-1".to_string()),
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let roundtrip_msg = recv_callback_timeout(&roundtrip, "get item reply").unwrap();
    mark_request_handled(&ctx, db_name, txn, 1);
    let commit_msg = recv_callback_timeout(&commit_result, "commit reply");
    assert_eq!(roundtrip_msg, Some(b"Moby Dick".to_vec()));
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readwrite,
        vec![store_name.to_string()],
    );
    let remove_result = remove_item(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyType::String("book-1".to_string()),
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let remove_msg = recv_callback_timeout(&remove_result, "remove item reply");
    mark_request_handled(&ctx, db_name, txn, 1);
    let commit_msg = recv_callback_timeout(&commit_result, "commit reply");
    assert!(remove_msg.is_ok());
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readonly,
        vec![store_name.to_string()],
    );
    let deleted = get_item(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyType::String("book-1".to_string()),
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let deleted_msg = recv_callback_timeout(&deleted, "get deleted item reply").unwrap();
    mark_request_handled(&ctx, db_name, txn, 1);
    let commit_msg = recv_callback_timeout(&commit_result, "commit reply");
    assert_eq!(deleted_msg, None);
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    close_database(&ctx, connection_id, db_name);
    let reopened_id = reopen_database(&ctx, db_name, store_name);
    assert_ne!(reopened_id, Uuid::nil());
    assert_eq!(get_version(&ctx, db_name), 1);
    let reopened_store = get_object_store(&ctx, db_name, store_name);
    assert_eq!(reopened_store.indexes.len(), 1);

    ctx.shutdown();
}

#[test]
fn test_indexeddb_store_names_support_nul_bytes() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-nul";
    let store_name = "books\0archive";

    let connection_id = expect_upgrade_and_initialize_database(
        &ctx,
        db_name,
        store_name,
        "by_author",
        Some(KeyPath::String("id".to_string())),
    );

    let store = get_object_store(&ctx, db_name, store_name);
    assert_eq!(store.name, store_name);

    close_database(&ctx, connection_id, db_name);
    let reopened_id = reopen_database(&ctx, db_name, store_name);
    assert_ne!(reopened_id, Uuid::nil());
    let reopened_store = get_object_store(&ctx, db_name, store_name);
    assert_eq!(reopened_store.name, store_name);

    ctx.shutdown();
}

#[test]
fn test_indexeddb_version_upgrade_persists_metadata() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-upgrade";
    let store_name = "books";

    let connection_id = expect_upgrade_and_initialize_database(
        &ctx,
        db_name,
        store_name,
        "by_author",
        Some(KeyPath::String("id".to_string())),
    );
    close_database(&ctx, connection_id, db_name);

    let receiver = open_database(&ctx, db_name, Some(2));
    let _upgraded_id = loop {
        match recv_callback_timeout(&receiver, "upgrade database reply") {
            ConnectionMsg::Upgrade {
                id: _,
                name,
                version,
                old_version,
                transaction,
                object_store_names,
            } => {
                assert_eq!(name, db_name);
                assert_eq!(old_version, 1);
                assert_eq!(version, 2);
                assert_eq!(object_store_names, vec![store_name.to_string()]);
                finish_upgrade_transaction(&ctx, transaction, db_name, true);
            },
            ConnectionMsg::Connection {
                id,
                name,
                version,
                upgraded,
                object_store_names,
            } => {
                assert_eq!(name, db_name);
                assert_eq!(version, 2);
                assert!(upgraded);
                assert_eq!(object_store_names, vec![store_name.to_string()]);
                break id;
            },
            other => panic!("unexpected upgrade message: {other:?}"),
        }
    };

    assert_eq!(get_version(&ctx, db_name), 2);
    ctx.shutdown();
}

#[test]
fn test_indexeddb_duplicate_store_reports_already_exists() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-duplicate-store";
    let connection_id = expect_upgrade_and_initialize_database(
        &ctx,
        db_name,
        "books",
        "by_author",
        Some(KeyPath::String("id".to_string())),
    );

    assert_eq!(
        create_object_store(
            &ctx,
            db_name,
            "books",
            Some(KeyPath::String("id".to_string())),
            false
        ),
        CreateObjectResult::AlreadyExists
    );

    close_database(&ctx, connection_id, db_name);
    ctx.shutdown();
}

fn abort_transaction(
    ctx: &IndexedDbTestContext,
    db_name: &str,
    txn: u64,
) -> mpsc::Receiver<Option<TxnCompleteMsg>> {
    let (sender, receiver) = make_profile_callback();
    GenericSend::send(
        &ctx.private_threads,
        IndexedDBThreadMsg::Sync(SyncOperation::Abort(
            sender,
            ctx.origin.clone(),
            db_name.to_string(),
            txn,
        )),
    )
    .unwrap();
    receiver
}

#[test]
fn test_indexeddb_aborted_transaction_restores_pre_transaction_state() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-abort-txn";
    let store_name = "books";
    let connection_id = expect_upgrade_and_initialize_database(
        &ctx,
        db_name,
        store_name,
        "by_author",
        Some(KeyPath::String("id".to_string())),
    );

    // Write inside a transaction, then abort instead of committing. The queued
    // request must never reach the backing store, so no reply is awaited on the
    // put callback.
    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readwrite,
        vec![store_name.to_string()],
    );
    let _put_result = put_item(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyType::String("book-aborted".to_string()),
        b"Never Committed".to_vec(),
    );
    let abort_result = abort_transaction(&ctx, db_name, txn);
    let abort_msg = recv_callback_timeout(&abort_result, "abort reply");
    assert_eq!(abort_msg.txn, txn);
    // An aborted transaction completes with Err(Abort); that error IS the
    // abort signal script uses to fire the abort event, not a failure.
    assert!(matches!(
        abort_msg.result,
        Err(storage_traits::indexeddb::BackendError::Abort)
    ));
    finish_transaction(&ctx, db_name, txn);

    // <https://w3c.github.io/IndexedDB/#abort-a-transaction>
    // "... the implementation must undo (roll back) any changes that were
    // made to the database during that transaction."
    // The pre-transaction state is restored: the key is absent.
    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readonly,
        vec![store_name.to_string()],
    );
    let lookup = get_item(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyType::String("book-aborted".to_string()),
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let lookup_msg = recv_callback_timeout(&lookup, "get aborted item reply").unwrap();
    mark_request_handled(&ctx, db_name, txn, 1);
    let commit_msg = recv_callback_timeout(&commit_result, "commit reply");
    assert_eq!(lookup_msg, None);
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    close_database(&ctx, connection_id, db_name);
    ctx.shutdown();
}

#[test]
fn test_indexeddb_aborted_upgrade_reverts_version_and_stores() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-abort-upgrade";
    let store_name = "books";
    let connection_id = expect_upgrade_and_initialize_database(
        &ctx,
        db_name,
        store_name,
        "by_author",
        Some(KeyPath::String("id".to_string())),
    );

    // Commit one row so the aborted upgrade can be shown to leave data intact.
    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readwrite,
        vec![store_name.to_string()],
    );
    let put_result = put_item(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyType::String("book-1".to_string()),
        b"Moby Dick".to_vec(),
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    recv_callback_timeout(&put_result, "put item reply").unwrap();
    mark_request_handled(&ctx, db_name, txn, 1);
    let commit_msg = recv_callback_timeout(&commit_result, "commit reply");
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);
    close_database(&ctx, connection_id, db_name);

    // Request an upgrade to version 2, create a store inside the upgrade
    // transaction, then finish it WITHOUT committing. Per
    // https://w3c.github.io/IndexedDB/#abort-an-upgrade-transaction the open
    // request is aborted and the database reverts to its pre-upgrade state.
    let receiver = open_database(&ctx, db_name, Some(2));
    let aborted;
    loop {
        match recv_callback_timeout(&receiver, "upgrade-to-v2 reply") {
            ConnectionMsg::Upgrade {
                id: _,
                name,
                version,
                old_version,
                transaction,
                object_store_names,
            } => {
                assert_eq!(name, db_name);
                assert_eq!(version, 2);
                assert_eq!(old_version, 1);
                assert_eq!(object_store_names, vec![store_name.to_string()]);
                assert_eq!(
                    create_object_store(&ctx, db_name, "phantom", None, false),
                    CreateObjectResult::Created
                );
                // Mirror script's abort choreography (idbtransaction.rs
                // finalize_abort): UpgradeTransactionFinished(committed=false)
                // first, then TransactionFinished to release the transaction's
                // scope in the scheduler.
                finish_upgrade_transaction(&ctx, transaction, db_name, false);
                finish_transaction(&ctx, db_name, transaction);
            },
            ConnectionMsg::AbortError { name, id: _ } => {
                assert_eq!(name, db_name);
                aborted = true;
                break;
            },
            other => panic!("unexpected upgrade-abort message: {other:?}"),
        }
    }
    assert!(aborted, "aborted upgrade must abort the open request");

    // Version reverted, the phantom store is gone, and only the original
    // store (with its data) remains.
    assert_eq!(get_version(&ctx, db_name), 1);
    let connection_id = reopen_database(&ctx, db_name, store_name);

    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readonly,
        vec![store_name.to_string()],
    );
    let lookup = get_item(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyType::String("book-1".to_string()),
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let lookup_msg = recv_callback_timeout(&lookup, "get preserved item reply").unwrap();
    mark_request_handled(&ctx, db_name, txn, 1);
    let commit_msg = recv_callback_timeout(&commit_result, "commit reply");
    assert_eq!(lookup_msg, Some(b"Moby Dick".to_vec()));
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    close_database(&ctx, connection_id, db_name);
    ctx.shutdown();
}

#[test]
fn test_indexeddb_cross_type_key_sort_order() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-sort-order";
    let store_name = "items";
    let connection_id =
        expect_upgrade_and_initialize_database(&ctx, db_name, store_name, "by_author", None);

    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readwrite,
        vec![store_name.to_string()],
    );
    let mut receivers = Vec::new();
    for (request_id, key, value) in [
        (1, IndexedDBKeyType::Number(1.0), b"number".to_vec()),
        (2, IndexedDBKeyType::Date(1.0), b"date".to_vec()),
        (
            3,
            IndexedDBKeyType::String("1".to_string()),
            b"string".to_vec(),
        ),
        (4, IndexedDBKeyType::Binary(vec![1]), b"binary".to_vec()),
        (
            5,
            IndexedDBKeyType::Array(vec![IndexedDBKeyType::Number(1.0)]),
            b"array".to_vec(),
        ),
    ] {
        receivers.push((
            request_id,
            put_item_request(
                &ctx,
                db_name,
                store_name,
                txn,
                request_id,
                Some(key),
                value,
                true,
            ),
        ));
    }
    let commit_result = commit_transaction(&ctx, db_name, txn);
    for (request_id, receiver) in receivers {
        let result = recv_callback_timeout(&receiver, "sorted put reply");
        assert!(result.is_ok(), "request {request_id} should succeed");
        mark_request_handled(&ctx, db_name, txn, request_id);
    }
    let commit_msg = recv_callback_timeout(&commit_result, "sorted commit reply");
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readonly,
        vec![store_name.to_string()],
    );
    let keys = get_all_keys_request(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyRange::default(),
        1,
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let keys = recv_callback_timeout(&keys, "sorted keys reply").unwrap();
    mark_request_handled(&ctx, db_name, txn, 1);
    let commit_msg = recv_callback_timeout(&commit_result, "sorted read commit reply");
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    assert_eq!(
        keys,
        vec![
            IndexedDBKeyType::Number(1.0),
            IndexedDBKeyType::Date(1.0),
            IndexedDBKeyType::String("1".to_string()),
            IndexedDBKeyType::Binary(vec![1]),
            IndexedDBKeyType::Array(vec![IndexedDBKeyType::Number(1.0)]),
        ]
    );

    close_database(&ctx, connection_id, db_name);
    ctx.shutdown();
}

#[test]
fn test_indexeddb_delete_item_range_respects_open_bounds() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-open-bounds";
    let store_name = "items";
    let connection_id =
        expect_upgrade_and_initialize_database(&ctx, db_name, store_name, "by_author", None);

    fn remaining_keys_after_delete(
        ctx: &IndexedDbTestContext,
        db_name: &str,
        store_name: &str,
        lower: i32,
        upper: i32,
        lower_open: bool,
        upper_open: bool,
    ) -> Vec<i32> {
        let txn = create_transaction(
            ctx,
            db_name,
            IndexedDBTxnMode::Readwrite,
            vec![store_name.to_string()],
        );
        let mut receivers = Vec::new();
        for key in 1..=10 {
            receivers.push((
                key,
                put_item_request(
                    ctx,
                    db_name,
                    store_name,
                    txn,
                    key as u64,
                    Some(IndexedDBKeyType::Number(key as f64)),
                    vec![key as u8],
                    true,
                ),
            ));
        }
        let commit_result = commit_transaction(ctx, db_name, txn);
        for (request_id, receiver) in receivers {
            let result = recv_callback_timeout(&receiver, "seed put reply");
            assert!(result.is_ok(), "seed request {request_id} should succeed");
            mark_request_handled(ctx, db_name, txn, request_id as u64);
        }
        let commit_msg = recv_callback_timeout(&commit_result, "seed commit reply");
        assert!(commit_msg.result.is_ok());
        finish_transaction(ctx, db_name, txn);

        let txn = create_transaction(
            ctx,
            db_name,
            IndexedDBTxnMode::Readwrite,
            vec![store_name.to_string()],
        );
        let delete = remove_item_request(
            ctx,
            db_name,
            store_name,
            txn,
            IndexedDBKeyRange::new(
                Some(IndexedDBKeyType::Number(lower as f64)),
                Some(IndexedDBKeyType::Number(upper as f64)),
                lower_open,
                upper_open,
            ),
        );
        let commit_result = commit_transaction(ctx, db_name, txn);
        let delete_msg = recv_callback_timeout(&delete, "range delete reply");
        assert!(delete_msg.is_ok());
        mark_request_handled(ctx, db_name, txn, 1);
        let commit_msg = recv_callback_timeout(&commit_result, "range delete commit reply");
        assert!(commit_msg.result.is_ok());
        finish_transaction(ctx, db_name, txn);

        let txn = create_transaction(
            ctx,
            db_name,
            IndexedDBTxnMode::Readonly,
            vec![store_name.to_string()],
        );
        let keys = get_all_keys_request(
            ctx,
            db_name,
            store_name,
            txn,
            IndexedDBKeyRange::default(),
            1,
        );
        let commit_result = commit_transaction(ctx, db_name, txn);
        let keys = recv_callback_timeout(&keys, "remaining keys reply").unwrap();
        mark_request_handled(ctx, db_name, txn, 1);
        let commit_msg = recv_callback_timeout(&commit_result, "remaining keys commit reply");
        assert!(commit_msg.result.is_ok());
        finish_transaction(ctx, db_name, txn);

        keys.into_iter()
            .map(|key| match key {
                IndexedDBKeyType::Number(number) => number as i32,
                other => panic!("Expected numeric key, got {other:?}"),
            })
            .collect()
    }

    assert_eq!(
        remaining_keys_after_delete(&ctx, db_name, store_name, 3, 8, false, false),
        vec![1, 2, 9, 10]
    );
    assert_eq!(
        remaining_keys_after_delete(&ctx, db_name, store_name, 3, 8, true, false),
        vec![1, 2, 3, 9, 10]
    );
    assert_eq!(
        remaining_keys_after_delete(&ctx, db_name, store_name, 3, 8, false, true),
        vec![1, 2, 8, 9, 10]
    );
    assert_eq!(
        remaining_keys_after_delete(&ctx, db_name, store_name, 3, 8, true, true),
        vec![1, 2, 3, 8, 9, 10]
    );

    close_database(&ctx, connection_id, db_name);
    ctx.shutdown();
}

#[test]
fn test_indexeddb_auto_increment_assigns_sequential_keys() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-auto-increment";
    let store_name = "items";

    let receiver = open_database(&ctx, db_name, Some(1));
    let connection_id = loop {
        match recv_callback_timeout(&receiver, "auto-increment open reply") {
            ConnectionMsg::Upgrade { transaction, .. } => {
                assert_eq!(
                    create_object_store(&ctx, db_name, store_name, None, true),
                    CreateObjectResult::Created
                );
                finish_upgrade_transaction(&ctx, transaction, db_name, true);
                finish_transaction(&ctx, db_name, transaction);
            },
            ConnectionMsg::Connection { id, upgraded, .. } => {
                assert!(upgraded);
                break id;
            },
            other => panic!("unexpected auto-increment message: {other:?}"),
        }
    };

    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readwrite,
        vec![store_name.to_string()],
    );
    let first = put_item_request(
        &ctx,
        db_name,
        store_name,
        txn,
        1,
        None,
        b"first".to_vec(),
        true,
    );
    let second = put_item_request(
        &ctx,
        db_name,
        store_name,
        txn,
        2,
        None,
        b"second".to_vec(),
        true,
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let first_msg = recv_callback_timeout(&first, "first auto-increment put reply").unwrap();
    let second_msg = recv_callback_timeout(&second, "second auto-increment put reply").unwrap();
    mark_request_handled(&ctx, db_name, txn, 1);
    mark_request_handled(&ctx, db_name, txn, 2);
    let commit_msg = recv_callback_timeout(&commit_result, "auto-increment commit reply");
    assert_eq!(first_msg, PutItemResult::Key(IndexedDBKeyType::Number(1.0)));
    assert_eq!(
        second_msg,
        PutItemResult::Key(IndexedDBKeyType::Number(2.0))
    );
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readonly,
        vec![store_name.to_string()],
    );
    let keys = get_all_keys_request(
        &ctx,
        db_name,
        store_name,
        txn,
        IndexedDBKeyRange::default(),
        1,
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let keys = recv_callback_timeout(&keys, "auto-increment keys reply").unwrap();
    mark_request_handled(&ctx, db_name, txn, 1);
    let commit_msg = recv_callback_timeout(&commit_result, "auto-increment read commit reply");
    assert!(commit_msg.result.is_ok());
    finish_transaction(&ctx, db_name, txn);

    assert_eq!(
        keys,
        vec![IndexedDBKeyType::Number(1.0), IndexedDBKeyType::Number(2.0),]
    );

    close_database(&ctx, connection_id, db_name);
    ctx.shutdown();
}

#[test]
fn test_indexeddb_batch_commit_failure_surfaces_an_error() {
    let ctx = IndexedDbTestContext::new();
    let db_name = "indexeddb-batch-failure";
    let store_name = "items";
    let connection_id =
        expect_upgrade_and_initialize_database(&ctx, db_name, store_name, "by_author", None);

    let txn = create_transaction(
        &ctx,
        db_name,
        IndexedDBTxnMode::Readwrite,
        vec![store_name.to_string()],
    );
    let failing = put_item_request(
        &ctx,
        db_name,
        store_name,
        txn,
        1,
        None,
        b"missing key".to_vec(),
        true,
    );
    let commit_result = commit_transaction(&ctx, db_name, txn);
    let failing_msg = recv_callback_timeout(&failing, "missing key batch put reply");
    assert!(failing_msg.is_err());
    mark_request_handled(&ctx, db_name, txn, 1);
    let _ = recv_callback_timeout(&commit_result, "batch commit reply");
    finish_transaction(&ctx, db_name, txn);

    close_database(&ctx, connection_id, db_name);
    ctx.shutdown();
}
