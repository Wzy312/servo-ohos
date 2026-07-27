/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(ohos_rdb)]

use std::fs;
use std::path::PathBuf;

use crate::ohos_rdb::{
    OhosRdbError, OhosRdbStore, OhosRdbTransaction, OhosRdbValues, Result as OhosRdbResult,
};
use servo_base::generic_channel::GenericSender;
use servo_base::id::{BrowsingContextId, WebViewId};
use servo_url::ImmutableOrigin;
use storage_traits::client_storage::{
    ClientStorageErrorr, ClientStorageThreadHandle, ClientStorageThreadMessage, Mode,
    StorageIdentifier, StorageProxyMap, StorageType,
};
use uuid::Uuid;

use crate::client_storage_shared::{self, StorageShelf};

use super::RegistryEngine;

const REGISTRY_FILE_NAME: &str = "reg.rdb";

pub(crate) struct OhosRdbEngine {
    store: OhosRdbStore,
    base_dir: PathBuf,
}

impl OhosRdbEngine {
    pub(crate) fn new(base_dir: PathBuf) -> OhosRdbResult<Self> {
        let store = OhosRdbStore::open(&base_dir, REGISTRY_FILE_NAME)?;
        Self::init(&store)?;

        Ok(Self { store, base_dir })
    }

    fn init(store: &OhosRdbStore) -> OhosRdbResult<()> {
        store.execute("PRAGMA foreign_keys = ON;")?;
        let tx = store.transaction()?;
        for sql in client_storage_shared::CLIENT_STORAGE_SCHEMA_SQL {
            execute_no_args(&tx, sql)?;
        }

        // TODO: Delete expired and non-persistent buckets on startup
        tx.commit()?;
        Ok(())
    }
}

fn missing_row(context: &'static str) -> OhosRdbError {
    OhosRdbError::Api { context, code: -1 }
}

fn opaque_origin_error(context: &'static str) -> OhosRdbError {
    OhosRdbError::Api { context, code: -1 }
}

fn execute_no_args(tx: &OhosRdbTransaction, sql: &str) -> OhosRdbResult<()> {
    let values = OhosRdbValues::new()?;
    tx.execute(sql, &values)
}

fn query_optional_text(
    tx: &OhosRdbTransaction,
    sql: &str,
    args: &OhosRdbValues,
) -> OhosRdbResult<Option<String>> {
    let mut cursor = tx.query_sql(sql, args)?;
    if !cursor.next_row()? {
        return Ok(None);
    }

    cursor.text(0)
}

fn ensure_storage_shed(
    storage_type: &StorageType,
    browsing_context: Option<String>,
    tx: &OhosRdbTransaction,
) -> OhosRdbResult<i64> {
    client_storage_shared::ensure_storage_shed(storage_type, browsing_context, tx, missing_row)
}

fn obtain_a_storage_shelf(
    shed: i64,
    origin: &ImmutableOrigin,
    storage_type: StorageType,
    tx: &OhosRdbTransaction,
) -> OhosRdbResult<StorageShelf> {
    client_storage_shared::obtain_a_storage_shelf(shed, origin, storage_type, tx, missing_row)
}

fn obtain_a_local_storage_shelf(
    origin: &ImmutableOrigin,
    tx: &OhosRdbTransaction,
) -> Result<StorageShelf, String> {
    if !origin.is_tuple() {
        return Err("Storage is unavailable for opaque origins".to_owned());
    }

    client_storage_shared::obtain_a_local_storage_shelf(
        origin,
        tx,
        missing_row,
        opaque_origin_error,
    )
    .map_err(|error| error.to_string())
}

fn bucket_mode(bucket_id: i64, tx: &OhosRdbTransaction) -> Result<Mode, String> {
    client_storage_shared::bucket_mode(bucket_id, tx, missing_row)
        .map_err(|error| error.to_string())
}

fn set_bucket_mode(bucket_id: i64, mode: Mode, tx: &OhosRdbTransaction) -> Result<(), String> {
    client_storage_shared::set_bucket_mode(bucket_id, mode, tx).map_err(|error| error.to_string())
}

fn storage_usage_for_bucket(bucket_id: i64, tx: &OhosRdbTransaction) -> Result<u64, String> {
    client_storage_shared::storage_usage_for_bucket(bucket_id, tx, |path| {
        client_storage_shared::directory_size(path).map_err(OhosRdbError::from)
    })
    .map_err(|error| error.to_string())
}

fn storage_quota_for_bucket(bucket_id: i64, tx: &OhosRdbTransaction) -> Result<u64, String> {
    client_storage_shared::storage_quota_for_bucket(bucket_id, tx)
        .map_err(|error| error.to_string())
}

fn changes(tx: &OhosRdbTransaction) -> OhosRdbResult<i64> {
    let args = OhosRdbValues::new()?;
    let mut cursor = tx.query_sql("SELECT changes();", &args)?;
    if !cursor.next_row()? {
        return Err(missing_row("OHOS RDB change count"));
    }

    cursor
        .int64(0)?
        .ok_or_else(|| missing_row("OHOS RDB change count"))
}

impl RegistryEngine for OhosRdbEngine {
    type Error = OhosRdbError;

    /// Create a database for the indexedDB endpoint.
    fn create_database(
        &mut self,
        bottle_id: i64,
        name: String,
    ) -> Result<(PathBuf, bool), ClientStorageErrorr<Self::Error>> {
        let tx = self
            .store
            .transaction()
            .map_err(ClientStorageErrorr::Internal)?;

        let mut args = OhosRdbValues::new().map_err(ClientStorageErrorr::Internal)?;
        args.push_int(bottle_id)
            .map_err(ClientStorageErrorr::Internal)?;
        args.push_text(name.as_str())
            .map_err(ClientStorageErrorr::Internal)?;
        tx.execute(
            "INSERT INTO databases (bottle_id, name) VALUES (?1, ?2) ON CONFLICT(bottle_id, name) DO UPDATE SET name = excluded.name;",
            &args,
        )
        .map_err(ClientStorageErrorr::Internal)?;

        let database_id = crate::client_storage_shared::query_required_i64(
            &tx,
            "SELECT id FROM databases WHERE bottle_id = ?1 AND name = ?2;",
            &args,
            "database lookup",
            missing_row,
        )
        .map_err(ClientStorageErrorr::Internal)?;

        let mut existing_path_args = OhosRdbValues::new().map_err(ClientStorageErrorr::Internal)?;
        existing_path_args
            .push_int(database_id)
            .map_err(ClientStorageErrorr::Internal)?;
        let existing_path = query_optional_text(
            &tx,
            "SELECT path FROM directories WHERE database_id = ?1;",
            &existing_path_args,
        )
        .map_err(ClientStorageErrorr::Internal)?;

        if let Some(p) = existing_path {
            // If it exists, we don't need the transaction anymore
            return Ok((PathBuf::from(p), false));
        }

        let dir = Uuid::new_v4().to_string();
        let cluster = dir.chars().last().unwrap();
        let path = self
            .base_dir
            .join("bottles")
            .join(cluster.to_string())
            .join(dir);

        let path_str = path.to_str().ok_or({
            ClientStorageErrorr::Internal(OhosRdbError::Api {
                context: "path",
                code: -1,
            })
        })?;

        let mut path_args = OhosRdbValues::new().map_err(ClientStorageErrorr::Internal)?;
        path_args
            .push_int(database_id)
            .map_err(ClientStorageErrorr::Internal)?;
        path_args
            .push_text(path_str)
            .map_err(ClientStorageErrorr::Internal)?;

        tx.execute(
            "INSERT INTO directories (database_id, path) VALUES (?1, ?2);",
            &path_args,
        )
        .map_err(ClientStorageErrorr::Internal)?;

        tx.commit().map_err(ClientStorageErrorr::Internal)?;

        std::fs::create_dir_all(&path).map_err(|_| ClientStorageErrorr::DirectoryCreationFailed)?;

        Ok((path, true))
    }

    /// Delete a database for the indexedDB endpoint.
    fn delete_database(
        &mut self,
        bottle_id: i64,
        name: String,
    ) -> Result<(), ClientStorageErrorr<Self::Error>> {
        let tx = self
            .store
            .transaction()
            .map_err(ClientStorageErrorr::Internal)?;

        let mut lookup_args = OhosRdbValues::new().map_err(ClientStorageErrorr::Internal)?;
        lookup_args
            .push_int(bottle_id)
            .map_err(ClientStorageErrorr::Internal)?;
        lookup_args
            .push_text(name.as_str())
            .map_err(ClientStorageErrorr::Internal)?;

        let database_id = crate::client_storage_shared::query_required_i64(
            &tx,
            "SELECT id FROM databases WHERE bottle_id = ?1 AND name = ?2;",
            &lookup_args,
            "database lookup",
            missing_row,
        )
        .map_err(ClientStorageErrorr::Internal)?;

        let mut path_args = OhosRdbValues::new().map_err(ClientStorageErrorr::Internal)?;
        path_args
            .push_int(database_id)
            .map_err(ClientStorageErrorr::Internal)?;

        let path = crate::client_storage_shared::query_required_text(
            &tx,
            "SELECT path FROM directories WHERE database_id = ?1;",
            &path_args,
            "directory lookup",
            missing_row,
        )
        .map_err(ClientStorageErrorr::Internal)?;

        tx.execute(
            "DELETE FROM directories WHERE database_id = ?1;",
            &path_args,
        )
        .map_err(ClientStorageErrorr::Internal)?;

        tx.execute(
            "DELETE FROM databases WHERE bottle_id = ?1 AND name = ?2;",
            &lookup_args,
        )
        .map_err(ClientStorageErrorr::Internal)?;

        if changes(&tx).map_err(ClientStorageErrorr::Internal)? == 0 {
            return Err(ClientStorageErrorr::DatabaseDoesNotExist);
        }

        tx.commit().map_err(ClientStorageErrorr::Internal)?;

        // Delete the directory on disk.
        // Note: on Windows this needs to be done outside of the transaction,
        // because the transaction holds a file lock.
        fs::remove_dir_all(&path).map_err(|_| ClientStorageErrorr::DirectoryDeletionFailed)?;

        Ok(())
    }

    /// <https://storage.spec.whatwg.org/#obtain-a-storage-bottle-map>
    fn obtain_a_storage_bottle_map(
        &mut self,
        storage_type: StorageType,
        webview: Option<WebViewId>,
        storage_identifier: StorageIdentifier,
        origin: ImmutableOrigin,
        sender: &GenericSender<ClientStorageThreadMessage>,
    ) -> Result<StorageProxyMap, ClientStorageErrorr<Self::Error>> {
        let tx = self
            .store
            .transaction()
            .map_err(ClientStorageErrorr::Internal)?;

        // Step 1. Let shed be null.
        let shed_id: i64 = match storage_type {
            StorageType::Local => {
                // Step 2. If type is "local", then set shed to the user agent’s storage shed.
                ensure_storage_shed(&storage_type, None, &tx)
                    .map_err(ClientStorageErrorr::Internal)?
            },
            StorageType::Session => {
                // Step 3: Otherwise:
                // Step 3.1: Assert: type is "session".
                let Some(webview) = webview else {
                    debug_assert!(false, "Session storage is only available on Window.");
                    return Err(ClientStorageErrorr::SessionStorageRequiresWindow);
                };

                // Step 3.2: Set shed to environment’s global object’s associated Document’s
                // node navigable’s traversable navigable’s storage shed.
                // Note: using the browsing context of the webview as the traversable navigable.
                ensure_storage_shed(
                    &storage_type,
                    Some(Into::<BrowsingContextId>::into(webview).to_string()),
                    &tx,
                )
                .map_err(ClientStorageErrorr::Internal)?
            },
        };

        // Step 4. Let shelf be the result of running obtain a storage shelf, with shed,
        // environment, and type.
        // Step 5. If shelf is failure, then return failure.
        let shelf = obtain_a_storage_shelf(shed_id, &origin, storage_type, &tx)
            .map_err(ClientStorageErrorr::Internal)?;

        // Step 6. Let bucket be shelf’s bucket map["default"].
        let bucket_id = shelf.default_bucket_id;

        let mut bottle_args = OhosRdbValues::new().map_err(ClientStorageErrorr::Internal)?;
        bottle_args
            .push_int(bucket_id)
            .map_err(ClientStorageErrorr::Internal)?;
        bottle_args
            .push_text(storage_identifier.as_str())
            .map_err(ClientStorageErrorr::Internal)?;

        let bottle_id = crate::client_storage_shared::query_required_i64(
            &tx,
            "SELECT id FROM bottles WHERE bucket_id = ?1 AND identifier = ?2;",
            &bottle_args,
            "bottle lookup",
            missing_row,
        )
        .map_err(ClientStorageErrorr::Internal)?;

        tx.commit().map_err(ClientStorageErrorr::Internal)?;

        // Step 7. Let bottle be bucket’s bottle map[identifier].

        // Step 8. Let proxyMap be a new storage proxy map whose backing map is bottle’s map.
        // Step 9. Append proxyMap to bottle’s proxy map reference set.
        // Step 10. Return proxyMap.
        Ok(StorageProxyMap {
            bottle_id,
            handle: ClientStorageThreadHandle::new(sender.clone()),
        })
    }

    fn persisted(&mut self, origin: ImmutableOrigin) -> Result<bool, String> {
        let tx = self
            .store
            .transaction()
            .map_err(|error| error.to_string())?;

        // <https://storage.spec.whatwg.org/#dom-storagemanager-persisted>
        // Let shelf be the result of running obtain a local storage shelf with this’s relevant
        // settings object.
        let shelf = obtain_a_local_storage_shelf(&origin, &tx)?;

        // Let persisted be true if shelf’s bucket map["default"]'s mode is "persistent";
        // otherwise false.
        // It will be false when there’s an internal error.
        let persisted = bucket_mode(shelf.default_bucket_id, &tx)
            .is_ok_and(|mode| mode == Mode::Persistent)
            && tx.commit().is_ok();

        Ok(persisted)
    }

    fn persist(
        &mut self,
        origin: ImmutableOrigin,
        permission_granted: bool,
    ) -> Result<bool, String> {
        let tx = self
            .store
            .transaction()
            .map_err(|error| error.to_string())?;

        // <https://storage.spec.whatwg.org/#dom-storagemanager-persist>
        // Let shelf be the result of running obtain a local storage shelf with this’s relevant
        // settings object.
        let shelf = obtain_a_local_storage_shelf(&origin, &tx)?;

        // Let bucket be shelf’s bucket map["default"].
        let bucket_id = shelf.default_bucket_id;

        // Let persisted be true if bucket’s mode is "persistent"; otherwise false.
        // It will be false when there’s an internal error.
        let mut persisted = bucket_mode(bucket_id, &tx).is_ok_and(|mode| mode == Mode::Persistent);

        // If persisted is false and permission is "granted", then:
        // Set bucket’s mode to "persistent".
        // If there was no internal error, then set persisted to true.
        if !persisted && permission_granted {
            persisted = set_bucket_mode(bucket_id, Mode::Persistent, &tx).is_ok();
        }

        if tx.commit().is_err() {
            persisted = false;
        }

        Ok(persisted)
    }

    fn estimate(&mut self, origin: ImmutableOrigin) -> Result<(u64, u64), String> {
        let tx = self
            .store
            .transaction()
            .map_err(|error| error.to_string())?;

        // <https://storage.spec.whatwg.org/#dom-storagemanager-estimate>
        // Let shelf be the result of running obtain a local storage shelf with this’s relevant
        // settings object.
        let shelf = obtain_a_local_storage_shelf(&origin, &tx)?;

        // Let usage be storage usage for shelf.
        let usage = storage_usage_for_bucket(shelf.default_bucket_id, &tx)?;
        // Let quota be storage quota for shelf.
        let quota = storage_quota_for_bucket(shelf.default_bucket_id, &tx)?;

        tx.commit().map_err(|error| error.to_string())?;

        Ok((usage, quota))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use servo_base::generic_channel::GenericCallback;
    use servo_base::id::{PIPELINE_NAMESPACE, PipelineNamespace, PipelineNamespaceId, WebViewId};
    use servo_url::ServoUrl;
    use tempfile::TempDir;

    use crate::ClientStorageThreadFactory;
    use storage_traits::client_storage::{
        ClientStorageThreadHandle, StorageIdentifier, StorageType,
    };

    use super::*;

    fn install_test_namespace() {
        if PIPELINE_NAMESPACE.get().is_none() {
            PipelineNamespace::install(PipelineNamespaceId(1));
        }
    }

    fn registry_db_path(tmp_dir: &TempDir) -> PathBuf {
        tmp_dir
            .path()
            .join("clientstorage")
            .join("default_v1")
            .join("rdb")
            .join(REGISTRY_FILE_NAME)
    }

    fn obtain_bottle_map(
        handle: &ClientStorageThreadHandle,
        storage_type: StorageType,
        webview: Option<WebViewId>,
        storage_identifier: StorageIdentifier,
        origin: servo_url::ImmutableOrigin,
    ) -> storage_traits::client_storage::StorageProxyMap {
        handle
            .obtain_a_storage_bottle_map(storage_type, webview, storage_identifier, origin)
            .recv()
            .unwrap()
            .unwrap()
    }

    #[cfg(ohos_rdb)]
    #[test]
    fn test_ohos_registry_uses_rdb_file() {
        install_test_namespace();
        let tmp_dir = tempfile::tempdir().unwrap();
        let handle: ClientStorageThreadHandle =
            ClientStorageThreadFactory::new(Some(tmp_dir.path().to_path_buf()), false);

        let url = ServoUrl::parse("https://example.com").unwrap();
        let storage_proxy_map = obtain_bottle_map(
            &handle,
            StorageType::Local,
            Some(WebViewId::new(servo_base::id::TEST_PAINTER_ID)),
            StorageIdentifier::IndexedDB,
            url.origin(),
        );

        let (path, created) = handle
            .create_database(storage_proxy_map.bottle_id, "ohos-rdb".to_string())
            .recv()
            .unwrap()
            .unwrap();

        assert!(created);
        assert!(path.is_dir());

        let registry_path = registry_db_path(&tmp_dir);
        assert_eq!(
            registry_path.file_name().and_then(|value| value.to_str()),
            Some("reg.rdb")
        );
        assert!(registry_path.exists());
    }

    #[cfg(ohos_rdb)]
    #[test]
    fn test_ohos_client_storage_roundtrip_uses_rdb_file() {
        install_test_namespace();
        let tmp_dir = tempfile::tempdir().unwrap();
        let handle: ClientStorageThreadHandle =
            ClientStorageThreadFactory::new(Some(tmp_dir.path().to_path_buf()), false);

        let origin = ServoUrl::parse("https://example.com").unwrap().origin();
        let storage_proxy_map = obtain_bottle_map(
            &handle,
            StorageType::Local,
            Some(WebViewId::new(servo_base::id::TEST_PAINTER_ID)),
            StorageIdentifier::IndexedDB,
            origin.clone(),
        );

        let (path, created) = handle
            .create_database(storage_proxy_map.bottle_id, "ohos-roundtrip".to_string())
            .recv()
            .unwrap()
            .unwrap();

        assert!(created);
        assert!(path.is_dir());

        let (cb, rx) = GenericCallback::new_blocking().unwrap();
        handle.persisted(origin.clone(), cb).unwrap();
        assert!(!rx.recv().unwrap().unwrap());

        let (cb, rx) = GenericCallback::new_blocking().unwrap();
        handle.persist(origin.clone(), true, cb).unwrap();
        assert!(rx.recv().unwrap().unwrap());

        let (cb, rx) = GenericCallback::new_blocking().unwrap();
        handle.estimate(origin, cb).unwrap();
        let (usage, quota) = rx.recv().unwrap().unwrap();
        assert!(quota > usage);

        let receiver = handle.delete_database(storage_proxy_map.bottle_id, "ohos-roundtrip".to_string());
        receiver.recv().unwrap().unwrap();

        let registry_path = registry_db_path(&tmp_dir);
        assert_eq!(
            registry_path.file_name().and_then(|value| value.to_str()),
            Some("reg.rdb")
        );
        assert!(registry_path.exists());
    }
}
