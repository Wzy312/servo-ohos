/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(ohos_rdb)]
//! Safe RAII wrappers around OHOS RDB handles.
//!
//! These wrappers own the native store, transaction, cursor, and values
//! handles. They centralize handle validation, keep the handles alive for the
//! lifetime of the Rust object, and destroy them exactly once in `Drop`.

mod cursor;
mod error;
mod params;

use std::ffi::CString;
use std::os::raw::c_int;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::NonNull;

pub(crate) use cursor::OhosRdbCursor;
pub(crate) use error::{OhosRdbError, Result};
use log::warn;
use ohos_rdb_sys::data_value::OH_Value_Destroy;
use ohos_rdb_sys::rdb_transaction::{
    OH_RDB_TransOptions, OH_RDB_TransType, OH_Rdb_Transaction, OH_RdbTrans_Commit,
    OH_RdbTrans_CreateOptions, OH_RdbTrans_Destroy, OH_RdbTrans_DestroyOptions,
    OH_RdbTrans_Execute, OH_RdbTrans_QuerySql, OH_RdbTrans_Rollback, OH_RdbTransOption_SetType,
};
use ohos_rdb_sys::relational_store::{
    OH_Rdb_CloseStore, OH_Rdb_ConfigV2, OH_Rdb_CreateConfig, OH_Rdb_CreateOrOpen,
    OH_Rdb_CreateTransaction, OH_Rdb_DestroyConfig, OH_Rdb_Execute, OH_Rdb_SecurityLevel,
    OH_Rdb_SetArea, OH_Rdb_SetDatabaseDir, OH_Rdb_SetDbType, OH_Rdb_SetSecurityLevel,
    OH_Rdb_SetStoreName, OH_Rdb_Store, Rdb_DBType, Rdb_SecurityArea,
};
use ohos_rdb_sys::relational_store_error_code::OH_Rdb_ErrCode;
pub(crate) use params::OhosRdbValues;

use self::error::{ensure_success, into_handle};
use crate::client_storage_shared::StorageSqlTransaction;

fn cstring_from_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(Into::into)
}

struct OhosRdbConfig {
    inner: NonNull<OH_Rdb_ConfigV2>,
}

impl OhosRdbConfig {
    fn new() -> Result<Self> {
        // SAFETY: The returned config handle is owned and validated by `into_handle`.
        unsafe {
            Ok(Self {
                inner: into_handle(OH_Rdb_CreateConfig(), "OH_Rdb_CreateConfig")?,
            })
        }
    }

    fn as_ptr(&self) -> *mut OH_Rdb_ConfigV2 {
        self.inner.as_ptr()
    }
}

impl Drop for OhosRdbConfig {
    fn drop(&mut self) {
        // SAFETY: The config handle belongs to `self` and is destroyed exactly once.
        unsafe {
            let _ = OH_Rdb_DestroyConfig(self.inner.as_ptr());
        }
    }
}

struct OhosRdbTransOptions {
    inner: NonNull<OH_RDB_TransOptions>,
}

impl OhosRdbTransOptions {
    fn new() -> Result<Self> {
        // SAFETY: The returned options handle is owned and validated by `into_handle`.
        unsafe {
            Ok(Self {
                inner: into_handle(OH_RdbTrans_CreateOptions(), "OH_RdbTrans_CreateOptions")?,
            })
        }
    }

    fn as_ptr(&self) -> *mut OH_RDB_TransOptions {
        self.inner.as_ptr()
    }
}

impl Drop for OhosRdbTransOptions {
    fn drop(&mut self) {
        // SAFETY: The options handle belongs to `self` and is destroyed exactly once.
        unsafe {
            let _ = OH_RdbTrans_DestroyOptions(self.inner.as_ptr());
        }
    }
}

#[derive(Debug)]
pub(crate) struct OhosRdbStore {
    inner: NonNull<OH_Rdb_Store>,
}

// SAFETY: `OH_Rdb_Store` is a movable owned handle whose native implementation
// is internally synchronized, so moving it across threads cannot introduce a
// data race:
// - `OH_Rdb_CreateOrOpen` resolves through `RdbStoreManager`'s path-keyed cache
//   to a shared `RdbStore` guarded by its own mutex
//   (rdb_store_manager.cpp, rdb_store_impl.cpp).
// - `OH_Rdb_CreateTransaction` hands each transaction a dedicated connection
//   from the store's transaction pool (`ConnPool::CreateTransConn`,
//   connection_pool.cpp), and every `OH_Rdb_Transaction` carries its own mutex
//   (transaction_impl.cpp). A transaction batch therefore never races the
//   store handle it was created from.
// (Sources: openharmony/distributeddatamgr_relational_store,
// OpenHarmony-6.0-Release — the release line the target devices run.)
// Engine code additionally serializes direct store calls (`execute`,
// `transaction` creation) behind `Mutex<OhosRdbStore>`; in-flight transactions
// intentionally outlive that guard and rely on the per-transaction connections
// described above.
unsafe impl Send for OhosRdbStore {}

impl OhosRdbStore {
    pub(crate) fn open(database_dir: &Path, store_name: &str) -> Result<Self> {
        let config = OhosRdbConfig::new()?;
        let database_dir = cstring_from_path(database_dir)?;
        let store_name = CString::new(store_name)?;

        // SAFETY: The config handle is initialized above and all FFI writes target local values.
        unsafe {
            ensure_success(
                OH_Rdb_SetDatabaseDir(config.as_ptr(), database_dir.as_ptr()),
                "OH_Rdb_SetDatabaseDir",
            )?;
            ensure_success(
                OH_Rdb_SetStoreName(config.as_ptr(), store_name.as_ptr()),
                "OH_Rdb_SetStoreName",
            )?;
            ensure_success(
                OH_Rdb_SetDbType(config.as_ptr(), Rdb_DBType::RDB_SQLITE.0 as c_int),
                "OH_Rdb_SetDbType",
            )?;
            // securityLevel is mandatory for OH_Rdb_ConfigV2; without it
            // OH_Rdb_CreateOrOpen fails with RDB_E_INVALID_ARGS (14800001).
            ensure_success(
                OH_Rdb_SetSecurityLevel(config.as_ptr(), OH_Rdb_SecurityLevel::S1.0 as c_int),
                "OH_Rdb_SetSecurityLevel",
            )?;
            // App storage lives in the EL2 (post-unlock) area.
            ensure_success(
                OH_Rdb_SetArea(
                    config.as_ptr(),
                    Rdb_SecurityArea::RDB_SECURITY_AREA_EL2.0 as c_int,
                ),
                "OH_Rdb_SetArea",
            )?;

            let mut err = 0;
            let raw = OH_Rdb_CreateOrOpen(config.as_ptr(), &mut err);
            if err != OH_Rdb_ErrCode::RDB_OK.0 {
                if let Some(raw) = NonNull::new(raw) {
                    let _ = OH_Rdb_CloseStore(raw.as_ptr());
                }
                return Err(OhosRdbError::Api {
                    context: "OH_Rdb_CreateOrOpen",
                    code: err,
                });
            }

            Self::from_raw(raw)
        }
    }

    /// # Safety
    ///
    /// `raw` must be a live `OH_Rdb_Store` returned by `OH_Rdb_CreateOrOpen`.
    /// This wrapper takes ownership and closes it in `Drop`.
    unsafe fn from_raw(raw: *mut OH_Rdb_Store) -> Result<Self> {
        Ok(Self {
            inner: into_handle(raw, "OH_Rdb_CreateOrOpen")?,
        })
    }

    pub(crate) fn transaction(&self) -> Result<OhosRdbTransaction> {
        let options = OhosRdbTransOptions::new()?;
        // SAFETY: The options handle is owned locally and the API fills only local pointers.
        unsafe {
            ensure_success(
                OH_RdbTransOption_SetType(options.as_ptr(), OH_RDB_TransType::RDB_TRANS_DEFERRED),
                "OH_RdbTransOption_SetType",
            )?;

            let mut raw = std::ptr::null_mut();
            let status = OH_Rdb_CreateTransaction(self.as_ptr(), options.as_ptr(), &mut raw);
            if status != OH_Rdb_ErrCode::RDB_OK.0 {
                if let Some(raw) = NonNull::new(raw) {
                    let _ = OH_RdbTrans_Destroy(raw.as_ptr());
                }
                return Err(OhosRdbError::Api {
                    context: "OH_Rdb_CreateTransaction",
                    code: status,
                });
            }

            OhosRdbTransaction::from_raw(raw)
        }
    }

    pub(crate) fn execute(&self, sql: &str) -> Result<()> {
        let sql = CString::new(sql)?;
        // SAFETY: The store handle is valid and the SQL string lives for the duration of the call.
        unsafe {
            ensure_success(
                OH_Rdb_Execute(self.as_ptr(), sql.as_ptr()),
                "OH_Rdb_Execute",
            )
        }
    }

    fn as_ptr(&self) -> *mut OH_Rdb_Store {
        self.inner.as_ptr()
    }
}

impl Drop for OhosRdbStore {
    fn drop(&mut self) {
        // SAFETY: The store handle belongs to `self` and is closed exactly once.
        unsafe {
            let _ = OH_Rdb_CloseStore(self.inner.as_ptr());
        }
    }
}

#[derive(Debug)]
pub(crate) struct OhosRdbTransaction {
    inner: Option<NonNull<OH_Rdb_Transaction>>,
}

impl OhosRdbTransaction {
    /// # Safety
    ///
    /// `raw` must be a live `OH_Rdb_Transaction` returned by
    /// `OH_Rdb_CreateTransaction`. This wrapper takes ownership and destroys it
    /// in `finish`.
    unsafe fn from_raw(raw: *mut OH_Rdb_Transaction) -> Result<Self> {
        Ok(Self {
            inner: Some(into_handle(raw, "OH_Rdb_CreateTransaction")?),
        })
    }

    fn as_ptr(&self) -> *mut OH_Rdb_Transaction {
        self.inner
            .as_ref()
            .expect("transaction should still be live")
            .as_ptr()
    }

    /// Runs a query and returns a cursor borrowed from this transaction.
    ///
    /// The borrow ties the cursor to the transaction, so finishing the
    /// transaction while a cursor is alive is rejected at compile time:
    ///
    /// ```compile_fail
    /// # // Shape only: committing consumes the transaction while the cursor
    /// # // still borrows it, which must not compile (native use-after-free).
    /// let tx = store.transaction()?;
    /// let mut cursor = tx.query_sql("SELECT 1;", &args)?;
    /// tx.commit()?;
    /// cursor.next_row()?;
    /// ```
    pub(crate) fn query_sql(&self, sql: &str, args: &OhosRdbValues) -> Result<OhosRdbCursor<'_>> {
        let sql = CString::new(sql)?;
        // SAFETY: The transaction handle is valid, the arguments outlive the
        // call, and the returned cursor's lifetime is pinned to `&self`.
        unsafe {
            OhosRdbCursor::from_raw(OH_RdbTrans_QuerySql(
                self.as_ptr(),
                sql.as_ptr(),
                args.as_ptr(),
            ))
        }
    }

    pub(crate) fn execute(&self, sql: &str, args: &OhosRdbValues) -> Result<()> {
        let sql = CString::new(sql)?;
        let mut result = std::ptr::null_mut();
        // SAFETY: The transaction handle is valid and `result` is a local out-parameter.
        unsafe {
            ensure_success(
                OH_RdbTrans_Execute(self.as_ptr(), sql.as_ptr(), args.as_ptr(), &mut result),
                "OH_RdbTrans_Execute",
            )?;
        }

        if let Some(result) = NonNull::new(result) {
            // SAFETY: The API returned an owned value handle that must be destroyed once.
            unsafe {
                ensure_success(OH_Value_Destroy(result.as_ptr()), "OH_Value_Destroy")?;
            }
        }

        Ok(())
    }

    pub(crate) fn commit(self) -> Result<()> {
        self.finish(OH_RdbTrans_Commit, "OH_RdbTrans_Commit")
    }

    pub(crate) fn rollback(self) -> Result<()> {
        self.finish(OH_RdbTrans_Rollback, "OH_RdbTrans_Rollback")
    }

    fn finish(
        mut self,
        action: unsafe extern "C" fn(*mut OH_Rdb_Transaction) -> c_int,
        context: &'static str,
    ) -> Result<()> {
        let ptr = self.inner.take().expect("transaction should still be live");
        // SAFETY: `ptr` is the live transaction handle consumed by this method.
        let action_status = unsafe { action(ptr.as_ptr()) };
        // SAFETY: The transaction handle is being torn down exactly once here.
        let destroy_status = unsafe { OH_RdbTrans_Destroy(ptr.as_ptr()) };

        if action_status != OH_Rdb_ErrCode::RDB_OK.0 {
            if destroy_status != OH_Rdb_ErrCode::RDB_OK.0 {
                warn!(
                    "OH_RdbTrans_Destroy failed after {} with status {}",
                    context, destroy_status
                );
            }
            return Err(OhosRdbError::Api {
                context,
                code: action_status,
            });
        }

        if destroy_status != OH_Rdb_ErrCode::RDB_OK.0 {
            warn!(
                "OH_RdbTrans_Destroy failed after {} with status {}",
                context, destroy_status
            );
        }

        Ok(())
    }
}

impl Drop for OhosRdbTransaction {
    fn drop(&mut self) {
        if let Some(ptr) = self.inner.take() {
            // SAFETY: The transaction handle belongs to `self` and is rolled back/destroyed once.
            unsafe {
                let _ = OH_RdbTrans_Rollback(ptr.as_ptr());
                let _ = OH_RdbTrans_Destroy(ptr.as_ptr());
            }
        }
    }
}

impl StorageSqlTransaction for OhosRdbTransaction {
    type Error = OhosRdbError;
    type Values = OhosRdbValues;

    fn new_values() -> Result<Self::Values> {
        OhosRdbValues::new()
    }

    fn push_text(values: &mut Self::Values, value: &str) -> Result<()> {
        values.push_text(value)
    }

    fn push_int(values: &mut Self::Values, value: i64) -> Result<()> {
        values.push_int(value)
    }

    fn push_blob(values: &mut Self::Values, value: &[u8]) -> Result<()> {
        values.push_blob(value)
    }

    fn push_null(values: &mut Self::Values) -> Result<()> {
        values.push_null()
    }

    fn value_count(values: &Self::Values) -> Result<usize> {
        values.count()
    }

    fn execute(&self, sql: &str, values: &Self::Values) -> Result<()> {
        OhosRdbTransaction::execute(self, sql, values)
    }

    fn query_optional_i64(&self, sql: &str, values: &Self::Values) -> Result<Option<i64>> {
        let mut cursor = OhosRdbTransaction::query_sql(self, sql, values)?;
        if !cursor.next_row()? {
            return Ok(None);
        }

        cursor.int64(0)
    }

    fn query_optional_text(&self, sql: &str, values: &Self::Values) -> Result<Option<String>> {
        let mut cursor = OhosRdbTransaction::query_sql(self, sql, values)?;
        if !cursor.next_row()? {
            return Ok(None);
        }

        cursor.text(0)
    }

    fn query_optional_blob(&self, sql: &str, values: &Self::Values) -> Result<Option<Vec<u8>>> {
        let mut cursor = OhosRdbTransaction::query_sql(self, sql, values)?;
        if !cursor.next_row()? {
            return Ok(None);
        }

        cursor.blob(0)
    }

    fn for_each_text<F>(&self, sql: &str, values: &Self::Values, mut f: F) -> Result<()>
    where
        F: FnMut(String) -> std::result::Result<(), Self::Error>,
    {
        let mut cursor = OhosRdbTransaction::query_sql(self, sql, values)?;
        while cursor.next_row()? {
            let Some(value) = cursor.text(0)? else {
                return Err(OhosRdbError::Api {
                    context: "OHOS RDB NULL text column",
                    code: -1,
                });
            };
            f(value)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::raw::{c_char, c_int, c_uchar};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    use ohos_rdb_sys::cursor::OH_Cursor;
    use ohos_rdb_sys::rdb_types::OH_ColumnType;
    use ohos_rdb_sys::relational_store_error_code::OH_Rdb_ErrCode;

    use super::*;

    #[test]
    fn binds_text_int_and_null_values() {
        let mut values = OhosRdbValues::new().unwrap();

        values.push_text("alpha").unwrap();
        values.push_int(7).unwrap();
        values.push_null().unwrap();

        assert_eq!(values.count().unwrap(), 3);
        assert_eq!(values.text(0).unwrap(), Some(String::from("alpha")));
        assert_eq!(values.int(1).unwrap(), Some(7));
        assert!(values.is_null(2).unwrap());
    }

    #[test]
    fn cursor_helpers_iterate_rows_and_decode_values() {
        DESTROYED.store(false, Ordering::SeqCst);
        ROW_INDEX.store(-1, Ordering::SeqCst);

        let raw = fake_cursor();
        let mut cursor = unsafe { OhosRdbCursor::from_raw(raw) }.unwrap();

        assert!(cursor.next_row().unwrap());
        assert_eq!(cursor.text(0).unwrap(), Some(String::from("a\0b")));
        assert_eq!(cursor.int64(1).unwrap(), Some(10));

        assert!(cursor.next_row().unwrap());
        assert_eq!(cursor.text(0).unwrap(), Some(String::from("bob")));
        assert_eq!(cursor.int64(1).unwrap(), Some(20));

        assert!(!cursor.next_row().unwrap());

        drop(cursor);
        assert!(DESTROYED.load(Ordering::SeqCst));
    }

    static DESTROYED: AtomicBool = AtomicBool::new(false);
    static ROW_INDEX: AtomicI32 = AtomicI32::new(-1);

    const TEXTS: [&str; 2] = ["a\0b", "bob"];
    const VALUES: [i64; 2] = [10, 20];

    unsafe extern "C" fn get_column_count(_: *mut OH_Cursor, count: *mut c_int) -> c_int {
        unsafe {
            *count = 2;
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_row_count(_: *mut OH_Cursor, count: *mut c_int) -> c_int {
        unsafe {
            *count = 2;
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_column_type(
        _: *mut OH_Cursor,
        column_index: i32,
        column_type: *mut OH_ColumnType,
    ) -> c_int {
        unsafe {
            *column_type = match column_index {
                0 => OH_ColumnType(4),
                1 => OH_ColumnType(1),
                _ => return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0,
            };
        }

        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn go_to_next_row(_: *mut OH_Cursor) -> c_int {
        let next = ROW_INDEX.fetch_add(1, Ordering::SeqCst) + 1;
        if next < 2 {
            OH_Rdb_ErrCode::RDB_OK.0
        } else {
            OH_Rdb_ErrCode::RDB_E_STEP_RESULT_IS_AFTER_LAST.0
        }
    }

    unsafe extern "C" fn get_size(_: *mut OH_Cursor, column_index: i32, size: *mut usize) -> c_int {
        if column_index != 0 {
            return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0;
        }

        let row = ROW_INDEX.load(Ordering::SeqCst).clamp(0, 1) as usize;
        unsafe {
            *size = TEXTS[row].len();
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_blob(
        _: *mut OH_Cursor,
        column_index: i32,
        value: *mut c_uchar,
        length: c_int,
    ) -> c_int {
        if column_index != 0 {
            return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0;
        }

        let row = ROW_INDEX.load(Ordering::SeqCst).clamp(0, 1) as usize;
        let bytes = TEXTS[row].as_bytes();
        assert_eq!(length as usize, bytes.len());
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_uchar, value, bytes.len());
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_int64(_: *mut OH_Cursor, column_index: i32, value: *mut i64) -> c_int {
        if column_index != 1 {
            return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0;
        }

        let row = ROW_INDEX.load(Ordering::SeqCst).clamp(0, 1) as usize;
        unsafe {
            *value = VALUES[row];
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn is_null(_: *mut OH_Cursor, _: i32, value: *mut bool) -> c_int {
        unsafe {
            *value = false;
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_column_name(
        _: *mut OH_Cursor,
        column_index: i32,
        value: *mut c_char,
        length: c_int,
    ) -> c_int {
        let name = match column_index {
            0 => "name",
            1 => "age",
            _ => return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0,
        };

        let bytes = name.as_bytes();
        assert!(length as usize >= bytes.len() + 1);
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, value, bytes.len());
            *value.add(bytes.len()) = 0;
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn destroy(cursor: *mut OH_Cursor) -> c_int {
        DESTROYED.store(true, Ordering::SeqCst);
        unsafe {
            drop(Box::from_raw(cursor));
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    fn fake_cursor() -> *mut OH_Cursor {
        Box::into_raw(Box::new(OH_Cursor {
            id: 1,
            getColumnCount: Some(get_column_count),
            getColumnType: Some(get_column_type),
            getColumnIndex: None,
            getColumnName: Some(get_column_name),
            getRowCount: Some(get_row_count),
            goToNextRow: Some(go_to_next_row),
            getSize: Some(get_size),
            getText: None,
            getInt64: Some(get_int64),
            getReal: None,
            getBlob: Some(get_blob),
            isNull: Some(is_null),
            destroy: Some(destroy),
            getAsset: None,
            getAssets: None,
        }))
    }
}

#[cfg(test)]
mod dispatch_tests {
    use std::os::raw::{c_char, c_int, c_uchar};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};

    use ohos_rdb_sys::cursor::OH_Cursor;
    use ohos_rdb_sys::rdb_types::OH_ColumnType;
    use ohos_rdb_sys::relational_store_error_code::OH_Rdb_ErrCode;

    use super::*;

    static DESTROYED: AtomicBool = AtomicBool::new(false);

    const TEXT_BYTES: &[u8] = b"alpha";
    const BLOB_BYTES: &[u8] = b"a\0b";

    #[test]
    fn text_dispatch_uses_column_type() {
        DESTROYED.store(false, Ordering::SeqCst);

        let raw = fake_cursor();
        let cursor = unsafe { OhosRdbCursor::from_raw(raw) }.unwrap();

        assert_eq!(cursor.text(0).unwrap(), Some(String::from("alpha")));
        assert_eq!(cursor.text(1).unwrap(), Some(String::from("a\0b")));

        drop(cursor);
        assert!(DESTROYED.load(Ordering::SeqCst));
    }

    unsafe extern "C" fn get_column_count(_: *mut OH_Cursor, count: *mut c_int) -> c_int {
        unsafe {
            *count = 2;
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_column_type(
        _: *mut OH_Cursor,
        column_index: i32,
        column_type: *mut OH_ColumnType,
    ) -> c_int {
        unsafe {
            *column_type = match column_index {
                0 => OH_ColumnType(3),
                1 => OH_ColumnType(4),
                _ => return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0,
            };
        }

        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_row_count(_: *mut OH_Cursor, count: *mut c_int) -> c_int {
        unsafe {
            *count = 1;
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn go_to_next_row(_: *mut OH_Cursor) -> c_int {
        OH_Rdb_ErrCode::RDB_E_STEP_RESULT_IS_AFTER_LAST.0
    }

    unsafe extern "C" fn get_size(_: *mut OH_Cursor, column_index: i32, size: *mut usize) -> c_int {
        unsafe {
            *size = match column_index {
                0 => TEXT_BYTES.len(),
                1 => BLOB_BYTES.len(),
                _ => return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0,
            };
        }

        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_text(
        _: *mut OH_Cursor,
        column_index: i32,
        value: *mut c_char,
        length: c_int,
    ) -> c_int {
        if column_index != 0 {
            return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0;
        }

        assert_eq!(length as usize, TEXT_BYTES.len() + 1);
        unsafe {
            ptr::copy_nonoverlapping(
                TEXT_BYTES.as_ptr() as *const c_char,
                value,
                TEXT_BYTES.len(),
            );
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_int64(_: *mut OH_Cursor, column_index: i32, value: *mut i64) -> c_int {
        if column_index != 1 {
            return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0;
        }

        unsafe {
            *value = 10;
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn get_blob(
        _: *mut OH_Cursor,
        column_index: i32,
        value: *mut c_uchar,
        length: c_int,
    ) -> c_int {
        if column_index != 1 {
            return OH_Rdb_ErrCode::RDB_E_INVALID_COLUMN_INDEX.0;
        }

        assert_eq!(length as usize, BLOB_BYTES.len());
        unsafe {
            ptr::copy_nonoverlapping(
                BLOB_BYTES.as_ptr() as *const c_uchar,
                value,
                BLOB_BYTES.len(),
            );
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn is_null(_: *mut OH_Cursor, _: i32, value: *mut bool) -> c_int {
        unsafe {
            *value = false;
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    unsafe extern "C" fn destroy(cursor: *mut OH_Cursor) -> c_int {
        DESTROYED.store(true, Ordering::SeqCst);
        unsafe {
            drop(Box::from_raw(cursor));
        }
        OH_Rdb_ErrCode::RDB_OK.0
    }

    fn fake_cursor() -> *mut OH_Cursor {
        Box::into_raw(Box::new(OH_Cursor {
            id: 2,
            getColumnCount: Some(get_column_count),
            getColumnType: Some(get_column_type),
            getColumnIndex: None,
            getColumnName: None,
            getRowCount: Some(get_row_count),
            goToNextRow: Some(go_to_next_row),
            getSize: Some(get_size),
            getText: Some(get_text),
            getInt64: Some(get_int64),
            getReal: None,
            getBlob: Some(get_blob),
            isNull: Some(is_null),
            destroy: Some(destroy),
            getAsset: None,
            getAssets: None,
        }))
    }
}

// Device-only concurrency regression test for the Send justification above:
// two store handles on the same database (which the native path-keyed cache
// resolves to one shared store) drive concurrent transactions, and every row
// must land. Runs on an OHOS device or emulator via
// `cargo test --target aarch64-unknown-linux-ohos --features ohos-rdb-backend`.
#[cfg(test)]
mod multihandle_spike {
    use std::path::Path;
    use std::thread;

    use super::*;

    fn insert_rows(store: &OhosRdbStore, range: std::ops::Range<i64>, tag: &str) {
        for i in range {
            // Concurrent writers may hit transient busy errors; retry a bounded
            // number of times. What the spike must prove is multi-handle
            // correctness, not busy-free execution.
            let mut attempts = 0;
            loop {
                attempts += 1;
                let result = (|| -> Result<()> {
                    let tx = store.transaction()?;
                    let mut vals = OhosRdbValues::new()?;
                    vals.push_int(i)?;
                    vals.push_text(&format!("{tag}{i}"))?;
                    tx.execute("INSERT OR REPLACE INTO t (k, v) VALUES (?, ?);", &vals)?;
                    tx.commit()
                })();
                match result {
                    Ok(()) => break,
                    Err(error) if attempts < 50 => {
                        eprintln!("[spike] retry {attempts} for k={i}: {error:?}");
                        thread::sleep(std::time::Duration::from_millis(10));
                    },
                    Err(error) => panic!("row k={i} failed after {attempts} attempts: {error:?}"),
                }
            }
        }
    }

    #[test]
    fn two_handles_same_db_concurrent_transactions() {
        let dir = Path::new("/data/local/tmp/rdb-spike");
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).expect("create spike dir");

        let store_a = OhosRdbStore::open(dir, "spike.db").expect("open handle A");
        // Question A: second handle on the same database.
        let store_b =
            OhosRdbStore::open(dir, "spike.db").expect("open handle B (multi-handle support)");

        store_a
            .execute("CREATE TABLE IF NOT EXISTS t (k INTEGER PRIMARY KEY, v TEXT);")
            .expect("create table");

        // Question B: concurrent transactions from the two handles.
        let writer_a = thread::spawn(move || insert_rows(&store_a, 0..50, "a"));
        let writer_b = thread::spawn(move || insert_rows(&store_b, 100..150, "b"));
        writer_a.join().expect("writer A");
        writer_b.join().expect("writer B");

        // A third handle verifies every row landed.
        let store_c = OhosRdbStore::open(dir, "spike.db").expect("open handle C");
        let tx = store_c.transaction().expect("verify tx");
        let mut cursor = tx
            .query_sql("SELECT COUNT(*) FROM t;", &OhosRdbValues::new().unwrap())
            .expect("count query");
        assert!(cursor.next_row().expect("count row"));
        assert_eq!(cursor.int64(0).expect("count value"), Some(100));
        drop(cursor);
        tx.commit().expect("verify commit");
        println!("[spike] PASS: multi-handle open + 100/100 concurrent rows landed");
    }
}
