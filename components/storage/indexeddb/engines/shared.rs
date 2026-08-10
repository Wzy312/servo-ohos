/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use storage_traits::indexeddb::{
    CreateObjectResult, IndexedDBIndex, IndexedDBKeyRange, IndexedDBKeyType, KeyPath, PutItemResult,
};

use crate::client_storage_shared::StorageSqlTransaction;
use crate::indexeddb::IndexedDBDescription;
use crate::indexeddb::engines::encoding;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObjectStoreModel {
    pub id: i64,
    pub name: String,
    pub key_path: Option<Vec<u8>>,
    pub auto_increment: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObjectStoreIndexModel {
    pub id: i64,
    pub object_store_id: i64,
    pub name: String,
    pub key_path: Vec<u8>,
    pub unique_index: bool,
    pub multi_entry_index: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObjectDataModel {
    pub object_store_id: i64,
    pub key: Vec<u8>,
    pub data: Vec<u8>,
}

pub(crate) const CREATE_DATABASE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS database (
    name    varchar          not null
        primary key,
    origin  varchar          not null,
    version bigint default 0 not null
) WITHOUT ROWID;"#;

pub(crate) const CREATE_OBJECT_STORE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS object_store (
    id             integer               not null
        primary key autoincrement,
    name           varchar               not null
        unique,
    key_path       varbinary_blob,
    auto_increment integer default FALSE not null
);"#;

pub(crate) const CREATE_OBJECT_DATA_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS object_data (
    object_store_id integer not null
        references object_store,
    key             blob    not null,
    data            blob    not null,
    constraint "pk-object_data"
        primary key (object_store_id, key)
) WITHOUT ROWID;"#;

pub(crate) const CREATE_OBJECT_STORE_INDEX_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS object_store_index (
    id                integer        not null
    primary key autoincrement,
    object_store_id   integer        not null
    references object_store,
    name              varchar        not null
    unique,
    key_path          varbinary_blob not null,
    unique_index      boolean        not null,
    multi_entry_index boolean        not null
);"#;

pub(crate) const CREATE_INDEX_DATA_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS index_data (
    index_id INTEGER NOT NULL,
    value BLOB NOT NULL,
    object_data_key BLOB NOT NULL,
    object_store_id INTEGER NOT NULL,
    value_locale BLOB,
    PRIMARY KEY (index_id, value, object_data_key)
    FOREIGN KEY (index_id) REFERENCES object_store_index(id),
    FOREIGN KEY (object_store_id, object_data_key)
    REFERENCES object_data(object_store_id, key)
) WITHOUT ROWID;"#;

pub(crate) const CREATE_UNIQUE_INDEX_DATA_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS unique_index_data (
    index_id INTEGER NOT NULL,
    value BLOB NOT NULL,
    object_store_id INTEGER NOT NULL,
    object_data_key BLOB NOT NULL,
    value_locale BLOB,
    PRIMARY KEY (index_id, value),
    FOREIGN KEY (index_id) REFERENCES object_store_index(id),
    FOREIGN KEY (object_store_id, object_data_key)
    REFERENCES object_data(object_store_id, key)
) WITHOUT ROWID;"#;

pub(crate) const CREATE_TABLES: [&str; 6] = [
    CREATE_DATABASE_TABLE,
    CREATE_OBJECT_STORE_TABLE,
    CREATE_OBJECT_DATA_TABLE,
    CREATE_OBJECT_STORE_INDEX_TABLE,
    CREATE_INDEX_DATA_TABLE,
    CREATE_UNIQUE_INDEX_DATA_TABLE,
];

pub(crate) fn encode_key_path(key_path: &KeyPath) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_stdvec(key_path)
}

pub(crate) fn decode_key_path<E>(
    data: &[u8],
    map_error: impl Fn(postcard::Error) -> E,
) -> Result<KeyPath, E> {
    postcard::from_bytes(data).map_err(map_error)
}

pub(crate) fn init_db<T: StorageSqlTransaction>(
    tx: &T,
    db_info: &IndexedDBDescription,
) -> Result<(), T::Error> {
    for stmt in CREATE_TABLES {
        let values = T::new_values()?;
        tx.execute(stmt, &values)?;
    }

    let values = T::new_values()?;
    if tx
        .query_optional_i64("SELECT version FROM database LIMIT 1;", &values)?
        .is_none()
    {
        let mut args = T::new_values()?;
        T::push_text(&mut args, &db_info.name)?;
        T::push_text(&mut args, &db_info.origin.to_owned().ascii_serialization())?;
        T::push_int(&mut args, 0)?;
        tx.execute(
            "INSERT INTO database (name, origin, version) VALUES (?1, ?2, ?3);",
            &args,
        )?;
    }

    Ok(())
}

pub(crate) fn object_store_by_name<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
) -> Result<Option<ObjectStoreModel>, T::Error> {
    let mut values = T::new_values()?;
    T::push_text(&mut values, store_name)?;

    let Some(id) = tx.query_optional_i64(
        "SELECT id FROM object_store WHERE name = ?1 LIMIT 1;",
        &values,
    )?
    else {
        return Ok(None);
    };
    let auto_increment = tx
        .query_optional_i64(
            "SELECT auto_increment FROM object_store WHERE name = ?1 LIMIT 1;",
            &values,
        )?
        .ok_or_else(|| missing_row("object_store.auto_increment"))?;
    let key_path = tx.query_optional_blob(
        "SELECT key_path FROM object_store WHERE name = ?1 LIMIT 1;",
        &values,
    )?;

    Ok(Some(ObjectStoreModel {
        id,
        name: store_name.to_owned(),
        key_path,
        auto_increment,
    }))
}

pub(crate) fn object_store_by_name_required<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
) -> Result<ObjectStoreModel, T::Error> {
    object_store_by_name(tx, store_name, missing_row)?
        .ok_or_else(|| missing_row("object store lookup"))
}

pub(crate) fn object_store_names<T: StorageSqlTransaction>(
    tx: &T,
) -> Result<Vec<String>, T::Error> {
    let values = T::new_values()?;
    let mut names = Vec::new();
    tx.for_each_text(
        "SELECT name FROM object_store ORDER BY id;",
        &values,
        |name| {
            names.push(name);
            Ok(())
        },
    )?;
    Ok(names)
}

pub(crate) fn object_store_index_by_name<T: StorageSqlTransaction>(
    tx: &T,
    store_id: i64,
    index_name: &str,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
) -> Result<Option<ObjectStoreIndexModel>, T::Error> {
    let mut values = T::new_values()?;
    T::push_int(&mut values, store_id)?;
    T::push_text(&mut values, index_name)?;

    let Some(id) = tx.query_optional_i64(
        "SELECT id FROM object_store_index WHERE object_store_id = ?1 AND name = ?2 LIMIT 1;",
        &values,
    )?
    else {
        return Ok(None);
    };
    let key_path = tx.query_optional_blob("SELECT key_path FROM object_store_index WHERE object_store_id = ?1 AND name = ?2 LIMIT 1;", &values)?.ok_or_else(|| missing_row("object_store_index.key_path"))?;
    let unique_index = tx.query_optional_i64("SELECT unique_index FROM object_store_index WHERE object_store_id = ?1 AND name = ?2 LIMIT 1;", &values)?.ok_or_else(|| missing_row("object_store_index.unique_index"))? != 0;
    let multi_entry_index = tx.query_optional_i64("SELECT multi_entry_index FROM object_store_index WHERE object_store_id = ?1 AND name = ?2 LIMIT 1;", &values)?.ok_or_else(|| missing_row("object_store_index.multi_entry_index"))? != 0;

    Ok(Some(ObjectStoreIndexModel {
        id,
        object_store_id: store_id,
        name: index_name.to_owned(),
        key_path,
        unique_index,
        multi_entry_index,
    }))
}

pub(crate) fn object_store_index_by_name_required<T: StorageSqlTransaction>(
    tx: &T,
    store_id: i64,
    index_name: &str,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
) -> Result<ObjectStoreIndexModel, T::Error> {
    object_store_index_by_name(tx, store_id, index_name, missing_row)?
        .ok_or_else(|| missing_row("object store index lookup"))
}

pub(crate) fn indexes<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
    map_key_path_error: impl Fn(postcard::Error) -> T::Error + Copy,
) -> Result<Vec<IndexedDBIndex>, T::Error> {
    let object_store = object_store_by_name_required(tx, store_name, missing_row)?;
    let mut values = T::new_values()?;
    T::push_int(&mut values, object_store.id)?;

    let mut names = Vec::new();
    tx.for_each_text(
        "SELECT name FROM object_store_index WHERE object_store_id = ?1 ORDER BY id;",
        &values,
        |name| {
            names.push(name);
            Ok(())
        },
    )?;

    names
        .into_iter()
        .map(|name| {
            let model =
                object_store_index_by_name_required(tx, object_store.id, &name, missing_row)?;
            Ok(IndexedDBIndex {
                name: model.name,
                key_path: decode_key_path(&model.key_path, map_key_path_error)?,
                unique: model.unique_index,
                multi_entry: model.multi_entry_index,
            })
        })
        .collect()
}

pub(crate) fn key_generator_current_number<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
) -> Result<Option<i64>, T::Error> {
    object_store_by_name(tx, store_name, missing_row).map(|opt| {
        opt.and_then(|store| (store.auto_increment != 0).then_some(store.auto_increment))
    })
}

fn ensure_rows_affected<T: StorageSqlTransaction>(
    tx: &T,
    expected: i64,
    context: &'static str,
    missing_row: impl Fn(&'static str) -> T::Error,
) -> Result<(), T::Error> {
    let values = T::new_values()?;
    let rows_affected = tx
        .query_optional_i64("SELECT changes();", &values)?
        .unwrap_or_default();
    if rows_affected == expected {
        Ok(())
    } else {
        Err(missing_row(context))
    }
}

pub(crate) fn set_key_generator_current_number<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    current_number: i64,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
) -> Result<(), T::Error> {
    let store = object_store_by_name_required(tx, store_name, missing_row)?;
    let mut values = T::new_values()?;
    T::push_int(&mut values, current_number)?;
    T::push_int(&mut values, store.id)?;
    tx.execute(
        "UPDATE object_store SET auto_increment = ?1 WHERE id = ?2;",
        &values,
    )?;
    ensure_rows_affected(tx, 1, "key generator update", missing_row)
}

pub(crate) fn key_path<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
    map_key_path_error: impl Fn(postcard::Error) -> T::Error + Copy,
) -> Result<Option<KeyPath>, T::Error> {
    match object_store_by_name(tx, store_name, missing_row)? {
        Some(store) => match store.key_path {
            Some(key_path) => decode_key_path(&key_path, map_key_path_error).map(Some),
            None => Ok(None),
        },
        None => Ok(None),
    }
}

pub(crate) fn create_store<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    key_path: Option<KeyPath>,
    auto_increment: bool,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
    map_key_path_error: impl Fn(postcard::Error) -> T::Error + Copy,
) -> Result<CreateObjectResult, T::Error> {
    if object_store_by_name(tx, store_name, missing_row)?.is_some() {
        return Ok(CreateObjectResult::AlreadyExists);
    }
    let mut values = T::new_values()?;
    T::push_text(&mut values, store_name)?;
    match key_path.as_ref() {
        Some(key_path) => {
            let encoded = encode_key_path(key_path).map_err(map_key_path_error)?;
            T::push_blob(&mut values, &encoded)?;
        },
        None => T::push_null(&mut values)?,
    }
    T::push_int(&mut values, auto_increment as i64)?;
    tx.execute(
        "INSERT INTO object_store (name, key_path, auto_increment) VALUES (?1, ?2, ?3);",
        &values,
    )?;
    Ok(CreateObjectResult::Created)
}

pub(crate) fn delete_store<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
) -> Result<(), T::Error> {
    let object_store = object_store_by_name_required(tx, store_name, missing_row)?;
    let mut values = T::new_values()?;
    T::push_int(&mut values, object_store.id)?;
    tx.execute(
        "DELETE FROM index_data WHERE object_store_id = ?1;",
        &values,
    )?;
    tx.execute(
        "DELETE FROM unique_index_data WHERE object_store_id = ?1;",
        &values,
    )?;
    tx.execute(
        "DELETE FROM object_store_index WHERE object_store_id = ?1;",
        &values,
    )?;
    tx.execute(
        "DELETE FROM object_data WHERE object_store_id = ?1;",
        &values,
    )?;
    tx.execute("DELETE FROM object_store WHERE id = ?1;", &values)?;
    ensure_rows_affected(tx, 1, "object store delete", missing_row)
}

pub(crate) fn create_index<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    index_name: String,
    key_path: KeyPath,
    unique: bool,
    multi_entry: bool,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
    map_key_path_error: impl Fn(postcard::Error) -> T::Error + Copy,
) -> Result<CreateObjectResult, T::Error> {
    let object_store = object_store_by_name_required(tx, store_name, missing_row)?;
    if object_store_index_by_name(tx, object_store.id, &index_name, missing_row)?.is_some() {
        return Ok(CreateObjectResult::AlreadyExists);
    }
    let mut values = T::new_values()?;
    T::push_int(&mut values, object_store.id)?;
    T::push_text(&mut values, &index_name)?;
    let encoded = encode_key_path(&key_path).map_err(map_key_path_error)?;
    T::push_blob(&mut values, &encoded)?;
    T::push_int(&mut values, unique as i64)?;
    T::push_int(&mut values, multi_entry as i64)?;
    tx.execute("INSERT INTO object_store_index (object_store_id, name, key_path, unique_index, multi_entry_index) VALUES (?1, ?2, ?3, ?4, ?5);", &values)?;
    Ok(CreateObjectResult::Created)
}

/// Renaming an index that does not exist is not an error, matching the SQLite twin.
pub(crate) fn rename_index<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    index_name: &str,
    new_name: &str,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
) -> Result<(), T::Error> {
    let object_store = object_store_by_name_required(tx, store_name, missing_row)?;
    let mut values = T::new_values()?;
    T::push_text(&mut values, new_name)?;
    T::push_text(&mut values, index_name)?;
    T::push_int(&mut values, object_store.id)?;
    tx.execute(
        "UPDATE object_store_index SET name = ?1 WHERE name = ?2 AND object_store_id = ?3;",
        &values,
    )
}

pub(crate) fn delete_index<T: StorageSqlTransaction>(
    tx: &T,
    store_name: &str,
    index_name: String,
    missing_row: impl Fn(&'static str) -> T::Error + Copy,
) -> Result<(), T::Error> {
    let object_store = object_store_by_name_required(tx, store_name, missing_row)?;
    let mut values = T::new_values()?;
    T::push_text(&mut values, &index_name)?;
    T::push_int(&mut values, object_store.id)?;
    tx.execute(
        "DELETE FROM object_store_index WHERE name = ?1 AND object_store_id = ?2;",
        &values,
    )
}

pub(crate) fn version<T: StorageSqlTransaction>(
    tx: &T,
    missing_row: impl Fn(&'static str) -> T::Error,
) -> Result<u64, T::Error> {
    let values = T::new_values()?;
    let version = tx
        .query_optional_i64("SELECT version FROM database LIMIT 1;", &values)?
        .ok_or_else(|| missing_row("database version lookup"))?;
    Ok(u64::from_ne_bytes(version.to_ne_bytes()))
}

pub(crate) fn set_version<T: StorageSqlTransaction>(
    tx: &T,
    version: u64,
    missing_row: impl Fn(&'static str) -> T::Error,
) -> Result<(), T::Error> {
    let values_exists = T::new_values()?;
    if tx
        .query_optional_i64("SELECT 1 FROM database LIMIT 1;", &values_exists)?
        .is_none()
    {
        return Err(missing_row("database version update"));
    }
    let mut values = T::new_values()?;
    T::push_int(&mut values, i64::from_ne_bytes(version.to_ne_bytes()))?;
    tx.execute("UPDATE database SET version = ?1;", &values)
}

fn build_range_clause<T: StorageSqlTransaction>(
    range: IndexedDBKeyRange,
    values: &mut T::Values,
    next_index: usize,
) -> Result<String, T::Error> {
    if let Some(singleton) = range.as_singleton() {
        let encoded = encoding::serialize(singleton);
        T::push_blob(values, &encoded)?;
        return Ok(format!("key = ?{next_index}"));
    }

    let mut clauses = Vec::new();
    let mut index = next_index;

    if let Some(upper) = range.upper.as_ref() {
        let encoded = encoding::serialize(upper);
        T::push_blob(values, &encoded)?;
        clauses.push(format!(
            "key {} ?{index}",
            if range.upper_open { "<" } else { "<=" }
        ));
        index += 1;
    }

    if let Some(lower) = range.lower.as_ref() {
        let encoded = encoding::serialize(lower);
        T::push_blob(values, &encoded)?;
        clauses.push(format!(
            "key {} ?{index}",
            if range.lower_open { ">" } else { ">=" }
        ));
    }

    Ok(if clauses.is_empty() {
        String::from("1")
    } else {
        clauses.join(" AND ")
    })
}

pub(crate) fn object_data_select_sql<T: StorageSqlTransaction>(
    select_list: &str,
    store_id: i64,
    key_range: IndexedDBKeyRange,
    count: Option<u32>,
    order_by_key: bool,
) -> Result<(String, T::Values), T::Error> {
    let mut values = T::new_values()?;
    T::push_int(&mut values, store_id)?;
    let range_clause = build_range_clause::<T>(key_range, &mut values, 2)?;

    let mut sql = format!(
        "SELECT {select_list} FROM object_data WHERE object_store_id = ?1 AND {range_clause}"
    );
    if order_by_key {
        sql.push_str(" ORDER BY key ASC");
    }
    if let Some(count) = count {
        T::push_int(&mut values, count as i64)?;
        sql.push_str(&format!(" LIMIT ?{}", T::value_count(&values)?));
    }
    sql.push(';');

    Ok((sql, values))
}

pub(crate) fn put_item<T: StorageSqlTransaction>(
    tx: &T,
    store: &ObjectStoreModel,
    key: IndexedDBKeyType,
    value: Vec<u8>,
    should_overwrite: bool,
    key_generator_current_number: Option<i64>,
) -> Result<PutItemResult, T::Error> {
    let no_overwrite = !should_overwrite;
    let serialized_key: Vec<u8> = encoding::serialize(&key);

    let mut exists_args = T::new_values()?;
    T::push_int(&mut exists_args, store.id)?;
    T::push_blob(&mut exists_args, &serialized_key)?;
    let existing_item = tx.query_optional_i64(
        "SELECT 1 FROM object_data WHERE object_store_id = ?1 AND key = ?2 LIMIT 1;",
        &exists_args,
    )?;

    if existing_item.is_some() {
        if no_overwrite {
            return Ok(PutItemResult::CannotOverwrite);
        }
        let mut update_args = T::new_values()?;
        T::push_blob(&mut update_args, &value)?;
        T::push_int(&mut update_args, store.id)?;
        T::push_blob(&mut update_args, &serialized_key)?;
        tx.execute(
            "UPDATE object_data SET data = ?1 WHERE object_store_id = ?2 AND key = ?3;",
            &update_args,
        )?;
    } else {
        let mut insert_args = T::new_values()?;
        T::push_int(&mut insert_args, store.id)?;
        T::push_blob(&mut insert_args, &serialized_key)?;
        T::push_blob(&mut insert_args, &value)?;
        tx.execute(
            "INSERT INTO object_data (object_store_id, key, data) VALUES (?1, ?2, ?3);",
            &insert_args,
        )?;
    }

    if let Some(next_key_generator_current_number) = key_generator_current_number {
        let mut update_args = T::new_values()?;
        T::push_int(&mut update_args, next_key_generator_current_number)?;
        T::push_int(&mut update_args, store.id)?;
        tx.execute(
            "UPDATE object_store SET auto_increment = ?1 WHERE id = ?2;",
            &update_args,
        )?;
    }

    Ok(PutItemResult::Key(key))
}

pub(crate) fn delete_item<T: StorageSqlTransaction>(
    tx: &T,
    store: &ObjectStoreModel,
    key_range: IndexedDBKeyRange,
) -> Result<(), T::Error> {
    let mut values = T::new_values()?;
    T::push_int(&mut values, store.id)?;
    let range_clause = build_range_clause::<T>(key_range, &mut values, 2)?;
    let sql = format!("DELETE FROM object_data WHERE object_store_id = ?1 AND {range_clause};");
    tx.execute(&sql, &values)
}

pub(crate) fn clear<T: StorageSqlTransaction>(
    tx: &T,
    store: &ObjectStoreModel,
) -> Result<(), T::Error> {
    let mut values = T::new_values()?;
    T::push_int(&mut values, store.id)?;
    tx.execute(
        "DELETE FROM object_data WHERE object_store_id = ?1;",
        &values,
    )
}

pub(crate) fn count<T: StorageSqlTransaction>(
    tx: &T,
    store: &ObjectStoreModel,
    key_range: IndexedDBKeyRange,
) -> Result<usize, T::Error> {
    let mut values = T::new_values()?;
    T::push_int(&mut values, store.id)?;
    let range_clause = build_range_clause::<T>(key_range, &mut values, 2)?;
    let sql =
        format!("SELECT COUNT(*) FROM object_data WHERE object_store_id = ?1 AND {range_clause};");
    tx.query_optional_i64(&sql, &values)
        .map(|count| count.unwrap_or_default() as usize)
}

#[cfg(test)]
mod tests {
    use storage_traits::indexeddb::KeyPath;

    use super::{decode_key_path, encode_key_path};

    #[test]
    fn key_path_round_trip() {
        let key_path = KeyPath::String("field".to_string());
        let encoded = encode_key_path(&key_path).unwrap();
        let decoded = decode_key_path(&encoded, |err| err).unwrap();
        assert_eq!(decoded, key_path);
    }

    #[test]
    fn key_path_decode_rejects_invalid_bytes() {
        assert!(decode_key_path(&[0xff], |err| err).is_err());
    }
}
