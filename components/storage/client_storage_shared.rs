/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

#[cfg(feature = "sqlite-backend")]
use rusqlite::OptionalExtension;

#[allow(dead_code)]
pub(crate) trait StorageSqlTransaction {
    type Error;
    type Values;

    fn new_values() -> Result<Self::Values, Self::Error>;
    fn push_text(values: &mut Self::Values, value: &str) -> Result<(), Self::Error>;
    fn push_int(values: &mut Self::Values, value: i64) -> Result<(), Self::Error>;
    fn push_blob(values: &mut Self::Values, value: &[u8]) -> Result<(), Self::Error>;
    fn push_null(values: &mut Self::Values) -> Result<(), Self::Error>;
    fn value_count(values: &Self::Values) -> Result<usize, Self::Error>;

    fn execute(&self, sql: &str, values: &Self::Values) -> Result<(), Self::Error>;
    fn query_optional_i64(
        &self,
        sql: &str,
        values: &Self::Values,
    ) -> Result<Option<i64>, Self::Error>;
    fn query_optional_text(
        &self,
        sql: &str,
        values: &Self::Values,
    ) -> Result<Option<String>, Self::Error>;
    fn query_optional_blob(
        &self,
        sql: &str,
        values: &Self::Values,
    ) -> Result<Option<Vec<u8>>, Self::Error>;
    fn for_each_text<F>(&self, sql: &str, values: &Self::Values, f: F) -> Result<(), Self::Error>
    where
        F: FnMut(String) -> Result<(), Self::Error>;
}

pub(crate) fn bind_optional_text<T: StorageSqlTransaction>(
    values: &mut T::Values,
    value: Option<&str>,
) -> Result<(), T::Error> {
    match value {
        Some(value) => T::push_text(values, value),
        None => T::push_null(values),
    }
}

pub(crate) fn query_required_i64<T: StorageSqlTransaction>(
    tx: &T,
    sql: &str,
    values: &T::Values,
    context: &'static str,
    missing_row: fn(&'static str) -> T::Error,
) -> Result<i64, T::Error> {
    tx.query_optional_i64(sql, values)?
        .ok_or_else(|| missing_row(context))
}

pub(crate) fn query_required_text<T: StorageSqlTransaction>(
    tx: &T,
    sql: &str,
    values: &T::Values,
    context: &'static str,
    missing_row: fn(&'static str) -> T::Error,
) -> Result<String, T::Error> {
    tx.query_optional_text(sql, values)?
        .ok_or_else(|| missing_row(context))
}

#[cfg(feature = "sqlite-backend")]
impl<'a> StorageSqlTransaction for rusqlite::Transaction<'a> {
    type Error = rusqlite::Error;
    type Values = Vec<rusqlite::types::Value>;

    fn new_values() -> Result<Self::Values, Self::Error> {
        Ok(Vec::new())
    }

    fn push_text(values: &mut Self::Values, value: &str) -> Result<(), Self::Error> {
        values.push(rusqlite::types::Value::Text(value.to_owned()));
        Ok(())
    }

    fn push_int(values: &mut Self::Values, value: i64) -> Result<(), Self::Error> {
        values.push(rusqlite::types::Value::Integer(value));
        Ok(())
    }

    fn push_blob(values: &mut Self::Values, value: &[u8]) -> Result<(), Self::Error> {
        values.push(rusqlite::types::Value::Blob(value.to_vec()));
        Ok(())
    }

    fn push_null(values: &mut Self::Values) -> Result<(), Self::Error> {
        values.push(rusqlite::types::Value::Null);
        Ok(())
    }

    fn value_count(values: &Self::Values) -> Result<usize, Self::Error> {
        Ok(values.len())
    }

    fn execute(&self, sql: &str, values: &Self::Values) -> Result<(), Self::Error> {
        rusqlite::Connection::execute(self, sql, rusqlite::params_from_iter(values.iter()))
            .map(|_| ())
    }

    fn query_optional_i64(
        &self,
        sql: &str,
        values: &Self::Values,
    ) -> Result<Option<i64>, Self::Error> {
        self.query_row(sql, rusqlite::params_from_iter(values.iter()), |row| {
            row.get(0)
        })
        .optional()
    }

    fn query_optional_text(
        &self,
        sql: &str,
        values: &Self::Values,
    ) -> Result<Option<String>, Self::Error> {
        self.query_row(sql, rusqlite::params_from_iter(values.iter()), |row| {
            row.get(0)
        })
        .optional()
    }

    fn query_optional_blob(
        &self,
        sql: &str,
        values: &Self::Values,
    ) -> Result<Option<Vec<u8>>, Self::Error> {
        self.query_row(sql, rusqlite::params_from_iter(values.iter()), |row| {
            row.get(0)
        })
        .optional()
    }

    fn for_each_text<F>(
        &self,
        sql: &str,
        values: &Self::Values,
        mut f: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(String) -> Result<(), Self::Error>,
    {
        let mut stmt = self.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), |row| {
            row.get::<_, String>(0)
        })?;
        for row in rows {
            f(row?)?;
        }
        Ok(())
    }
}
use servo_url::ImmutableOrigin;
use storage_traits::client_storage::{Mode, StorageIdentifier, StorageType};

/// <https://storage.spec.whatwg.org/#storage-quota>
pub(crate) const STORAGE_SHELF_QUOTA_BYTES: u64 = 10 * 1024 * 1024 * 1024;

pub(crate) const CLIENT_STORAGE_SCHEMA_SQL: [&str; 11] = [
    r#"CREATE TABLE IF NOT EXISTS sheds (
            id INTEGER PRIMARY KEY,
            storage_type TEXT NOT NULL,
            browsing_context TEXT
        );"#,
    r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_sheds_local
        ON sheds(storage_type) WHERE browsing_context IS NULL;"#,
    r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_sheds_session
        ON sheds(browsing_context) WHERE browsing_context IS NOT NULL;"#,
    r#"CREATE TABLE IF NOT EXISTS shelves (
            id INTEGER PRIMARY KEY,
            shed_id INTEGER NOT NULL,
            origin TEXT NOT NULL,
            UNIQUE (shed_id, origin),
            FOREIGN KEY (shed_id) REFERENCES sheds(id) ON DELETE CASCADE
        );"#,
    r#"CREATE TABLE IF NOT EXISTS buckets (
            id INTEGER PRIMARY KEY,
            shelf_id INTEGER NOT NULL UNIQUE,
            persisted BOOLEAN DEFAULT 0,
            name TEXT,
            mode TEXT,
            expires DATETIME,
            FOREIGN KEY (shelf_id) REFERENCES shelves(id) ON DELETE CASCADE
        );"#,
    r#"CREATE TABLE IF NOT EXISTS bottles (
                    id INTEGER PRIMARY KEY,
                    bucket_id INTEGER NOT NULL,
                    identifier TEXT NOT NULL,
                    UNIQUE (bucket_id, identifier),
                    FOREIGN KEY (bucket_id) REFERENCES buckets(id) ON DELETE CASCADE
                );"#,
    r#"CREATE TABLE IF NOT EXISTS databases (
                    id INTEGER PRIMARY KEY,
                    bottle_id INTEGER NOT NULL,
                    name TEXT NOT NULL,
                    UNIQUE (bottle_id, name),
                    FOREIGN KEY (bottle_id) REFERENCES bottles(id) ON DELETE CASCADE
                );
                "#,
    r#"CREATE TABLE IF NOT EXISTS directories (
                id INTEGER PRIMARY KEY,
                database_id INTEGER NOT NULL UNIQUE,
                path TEXT NOT NULL,
                FOREIGN KEY (database_id) REFERENCES databases(id) ON DELETE CASCADE
            );"#,
    r#"CREATE UNIQUE INDEX IF NOT EXISTS sheds_local_identity_idx
                ON sheds(storage_type)
                WHERE storage_type = 'local' AND browsing_context IS NULL;"#,
    r#"CREATE UNIQUE INDEX IF NOT EXISTS sheds_session_identity_idx
                ON sheds(storage_type, browsing_context)
                WHERE storage_type = 'session' AND browsing_context IS NOT NULL;"#,
    r#"CREATE UNIQUE INDEX IF NOT EXISTS shelves_origin_shed_identity_idx
                ON shelves(origin, shed_id);"#,
];

#[derive(Debug, Clone, Copy)]
pub(crate) struct StorageShelf {
    pub(crate) default_bucket_id: i64,
}

pub(crate) fn directory_size(path: &PathBuf) -> Result<u64, std::io::Error> {
    let metadata = fs::metadata(path)?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }

    if !metadata.is_dir() {
        return Ok(0);
    }

    let mut size = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        size += directory_size(&entry.path())?;
    }
    Ok(size)
}

pub(crate) fn ensure_storage_shed<T: StorageSqlTransaction>(
    storage_type: &StorageType,
    browsing_context: Option<String>,
    tx: &T,
    missing_row: fn(&'static str) -> T::Error,
) -> Result<i64, T::Error> {
    match browsing_context {
        Some(browsing_context) => {
            let mut args = T::new_values()?;
            T::push_text(&mut args, storage_type.as_str())?;
            bind_optional_text::<T>(&mut args, Some(browsing_context.as_str()))?;
            tx.execute(
                "INSERT INTO sheds (storage_type, browsing_context) VALUES (?1, ?2) ON CONFLICT DO NOTHING;",
                &args,
            )?;

            query_required_i64(
                tx,
                "SELECT id FROM sheds WHERE storage_type = ?1 AND browsing_context = ?2;",
                &args,
                "storage shed lookup",
                missing_row,
            )
        },
        None => {
            let mut args = T::new_values()?;
            T::push_text(&mut args, storage_type.as_str())?;
            tx.execute(
                "INSERT INTO sheds (storage_type, browsing_context) VALUES (?1, NULL) ON CONFLICT DO NOTHING;",
                &args,
            )?;

            query_required_i64(
                tx,
                "SELECT id FROM sheds WHERE storage_type = ?1 AND browsing_context IS NULL;",
                &args,
                "storage shed lookup",
                missing_row,
            )
        },
    }
}

/// <https://storage.spec.whatwg.org/#create-a-storage-bucket>
pub(crate) fn create_a_storage_bucket<T: StorageSqlTransaction>(
    shelf_id: i64,
    storage_type: StorageType,
    tx: &T,
    missing_row: fn(&'static str) -> T::Error,
) -> Result<i64, T::Error> {
    let bucket_id: i64 = if let StorageType::Local = storage_type {
        let mut args = T::new_values()?;
        T::push_text(&mut args, Mode::default().as_str())?;
        T::push_int(&mut args, shelf_id)?;
        tx.execute(
            "INSERT INTO buckets (mode, shelf_id) VALUES (?1, ?2) ON CONFLICT(shelf_id) DO NOTHING;",
            &args,
        )?;

        let mut select_args = T::new_values()?;
        T::push_int(&mut select_args, shelf_id)?;
        query_required_i64(
            tx,
            "SELECT id FROM buckets WHERE shelf_id = ?1;",
            &select_args,
            "bucket lookup",
            missing_row,
        )?
    } else {
        let mut args = T::new_values()?;
        T::push_int(&mut args, shelf_id)?;
        tx.execute(
            "INSERT INTO buckets (shelf_id) VALUES (?1) ON CONFLICT(shelf_id) DO NOTHING;",
            &args,
        )?;

        query_required_i64(
            tx,
            "SELECT id FROM buckets WHERE shelf_id = ?1;",
            &args,
            "bucket lookup",
            missing_row,
        )?
    };

    let registered_endpoints = match storage_type {
        StorageType::Local => vec![
            StorageIdentifier::Caches,
            StorageIdentifier::IndexedDB,
            StorageIdentifier::LocalStorage,
            StorageIdentifier::ServiceWorkerRegistrations,
        ],
        StorageType::Session => vec![StorageIdentifier::SessionStorage],
    };

    for identifier in registered_endpoints {
        let mut args = T::new_values()?;
        T::push_int(&mut args, bucket_id)?;
        T::push_text(&mut args, identifier.as_str())?;
        tx.execute(
            "INSERT INTO bottles (bucket_id, identifier) VALUES (?1, ?2) ON CONFLICT(bucket_id, identifier) DO NOTHING;",
            &args,
        )?;
    }

    Ok(bucket_id)
}

/// <https://storage.spec.whatwg.org/#create-a-storage-shelf>
pub(crate) fn create_a_storage_shelf<T: StorageSqlTransaction>(
    shed: i64,
    origin: &ImmutableOrigin,
    storage_type: StorageType,
    tx: &T,
    missing_row: fn(&'static str) -> T::Error,
) -> Result<StorageShelf, T::Error> {
    let mut args = T::new_values()?;
    T::push_int(&mut args, shed)?;
    let origin_serialized = origin.ascii_serialization();
    T::push_text(&mut args, origin_serialized.as_str())?;
    tx.execute(
        "INSERT INTO shelves (shed_id, origin) VALUES (?1, ?2) ON CONFLICT(shed_id, origin) DO NOTHING;",
        &args,
    )?;

    let shelf_id = query_required_i64(
        tx,
        "SELECT id FROM shelves WHERE shed_id = ?1 AND origin = ?2;",
        &args,
        "storage shelf lookup",
        missing_row,
    )?;

    Ok(StorageShelf {
        default_bucket_id: create_a_storage_bucket(shelf_id, storage_type, tx, missing_row)?,
    })
}

/// <https://storage.spec.whatwg.org/#obtain-a-storage-shelf>
pub(crate) fn obtain_a_storage_shelf<T: StorageSqlTransaction>(
    shed: i64,
    origin: &ImmutableOrigin,
    storage_type: StorageType,
    tx: &T,
    missing_row: fn(&'static str) -> T::Error,
) -> Result<StorageShelf, T::Error> {
    create_a_storage_shelf(shed, origin, storage_type, tx, missing_row)
}

pub(crate) fn obtain_a_local_storage_shelf<T: StorageSqlTransaction>(
    origin: &ImmutableOrigin,
    tx: &T,
    missing_row: fn(&'static str) -> T::Error,
    opaque_origin_error: fn(&'static str) -> T::Error,
) -> Result<StorageShelf, T::Error> {
    if !origin.is_tuple() {
        return Err(opaque_origin_error(
            "Storage is unavailable for opaque origins",
        ));
    }

    let shed = ensure_storage_shed(&StorageType::Local, None, tx, missing_row)?;
    obtain_a_storage_shelf(shed, origin, StorageType::Local, tx, missing_row)
}

pub(crate) fn bucket_mode<T: StorageSqlTransaction>(
    bucket_id: i64,
    tx: &T,
    missing_row: fn(&'static str) -> T::Error,
) -> Result<Mode, T::Error> {
    let mut args = T::new_values()?;
    T::push_int(&mut args, bucket_id)?;
    let mode = query_required_text(
        tx,
        "SELECT mode FROM buckets WHERE id = ?1;",
        &args,
        "bucket lookup",
        missing_row,
    )?;
    Mode::from_str(&mode).map_err(|_| missing_row("bucket mode parse"))
}

pub(crate) fn set_bucket_mode<T: StorageSqlTransaction>(
    bucket_id: i64,
    mode: Mode,
    tx: &T,
) -> Result<(), T::Error> {
    let mut args = T::new_values()?;
    T::push_text(&mut args, mode.as_str())?;
    T::push_int(
        &mut args,
        if matches!(mode, Mode::Persistent) {
            1
        } else {
            0
        },
    )?;
    T::push_int(&mut args, bucket_id)?;

    tx.execute(
        "UPDATE buckets SET mode = ?1, persisted = ?2 WHERE id = ?3;",
        &args,
    )
}

pub(crate) fn storage_usage_for_bucket<T, F>(
    bucket_id: i64,
    tx: &T,
    mut directory_size: F,
) -> Result<u64, T::Error>
where
    T: StorageSqlTransaction,
    F: FnMut(&PathBuf) -> Result<u64, T::Error>,
{
    let mut usage = 0_u64;
    let mut args = T::new_values()?;
    T::push_int(&mut args, bucket_id)?;

    tx.for_each_text(
        "SELECT directories.path
         FROM directories
         JOIN databases ON directories.database_id = databases.id
         JOIN bottles ON databases.bottle_id = bottles.id
         WHERE bottles.bucket_id = ?1
         ORDER BY directories.id;",
        &args,
        |path| {
            usage += directory_size(&PathBuf::from(path))?;
            Ok(())
        },
    )?;

    Ok(usage)
}

pub(crate) fn storage_quota_for_bucket<T: StorageSqlTransaction>(
    _bucket_id: i64,
    _tx: &T,
) -> Result<u64, T::Error> {
    Ok(STORAGE_SHELF_QUOTA_BYTES)
}

#[cfg(all(test, feature = "sqlite-backend"))]
mod tests {
    use rusqlite::Connection;

    use super::*;

    fn missing(_context: &'static str) -> rusqlite::Error {
        rusqlite::Error::QueryReturnedNoRows
    }

    fn connection_with_bucket_mode(mode: &str) -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE buckets (id INTEGER PRIMARY KEY, mode TEXT);")
            .unwrap();
        connection
            .execute("INSERT INTO buckets (id, mode) VALUES (1, ?1);", [mode])
            .unwrap();
        connection
    }

    #[test]
    fn bucket_mode_parses_stored_modes() {
        let mut connection = connection_with_bucket_mode("persistent");
        let tx = connection.transaction().unwrap();
        assert_eq!(bucket_mode(1, &tx, missing).unwrap(), Mode::Persistent);
        drop(tx);

        let mut connection = connection_with_bucket_mode("best-effort");
        let tx = connection.transaction().unwrap();
        assert_eq!(bucket_mode(1, &tx, missing).unwrap(), Mode::BestEffort);
    }

    #[test]
    fn bucket_mode_rejects_unparseable_mode() {
        let mut connection = connection_with_bucket_mode("garbage-mode");
        let tx = connection.transaction().unwrap();
        assert!(bucket_mode(1, &tx, missing).is_err());
    }
}
