/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(ohos_rdb)]

use std::os::raw::{c_char, c_int, c_uchar};
use std::ptr::NonNull;

use ohos_rdb_sys::cursor::OH_Cursor;
use ohos_rdb_sys::rdb_types::OH_ColumnType;
use ohos_rdb_sys::relational_store_error_code::OH_Rdb_ErrCode;

use super::error::{Result, ensure_success, into_handle, missing_method};
use crate::blob_text::{ColumnKind, decode_column};

macro_rules! cursor_call {
    ($self:expr, $field:ident $(, $arg:expr )* $(,)?) => {{
        let cursor = $self.inner.as_ptr();
        // SAFETY: `cursor` is a live handle owned by `self`, and this only reads a function pointer.
        let function = unsafe { (*cursor).$field }
            .ok_or_else(|| missing_method(stringify!($field)))?;
        // SAFETY: The callback belongs to this live cursor and the arguments match the native API.
        unsafe { function(cursor, $($arg),*) }
    }};
}

const BLOB_COLUMN_TYPE: OH_ColumnType = OH_ColumnType(4);

/// A row cursor borrowed from the transaction that produced it.
///
/// The `'trans` lifetime ties the cursor to that transaction: committing,
/// rolling back, or dropping the transaction destroys the native transaction
/// (and with it the cursor's backing statement), so the borrow checker must
/// reject any use of the cursor past that point.
#[derive(Debug)]
pub(crate) struct OhosRdbCursor<'trans> {
    inner: NonNull<OH_Cursor>,
    _transaction: std::marker::PhantomData<&'trans ()>,
}

impl<'trans> OhosRdbCursor<'trans> {
    /// # Safety
    ///
    /// `raw` must be a live cursor returned by `OH_RdbTrans_QuerySql`, the
    /// wrapper takes ownership of its destruction, and the caller must pin
    /// `'trans` to the transaction that produced `raw`.
    pub(crate) unsafe fn from_raw(raw: *mut OH_Cursor) -> Result<Self> {
        let inner = into_handle(raw, "OH_RdbTrans_QuerySql")?;
        // SAFETY: `inner` is a live cursor handle; reading the callback pointer is side-effect free.
        let destroy = unsafe { (*inner.as_ptr()).destroy };
        if destroy.is_none() {
            return Err(missing_method("destroy"));
        }

        Ok(Self {
            inner,
            _transaction: std::marker::PhantomData,
        })
    }

    fn column_type(&self, index: usize) -> Result<OH_ColumnType> {
        let mut kind = OH_ColumnType(0);
        // SAFETY: The native cursor is owned by `self` and writes the column type into `kind`.
        ensure_success(
            cursor_call!(self, getColumnType, index as i32, &mut kind),
            "OH_Cursor_GetColumnType",
        )?;
        Ok(kind)
    }

    pub(crate) fn next_row(&mut self) -> Result<bool> {
        // SAFETY: Advancing the native cursor only mutates cursor-local state.
        let status = cursor_call!(self, goToNextRow);
        if status == OH_Rdb_ErrCode::RDB_OK.0 {
            Ok(true)
        } else if status == OH_Rdb_ErrCode::RDB_E_STEP_RESULT_IS_AFTER_LAST.0 {
            Ok(false)
        } else {
            Err(super::OhosRdbError::Api {
                context: "OH_Cursor_GoToNextRow",
                code: status,
            })
        }
    }

    pub(crate) fn size(&self, index: usize) -> Result<usize> {
        let mut size = 0usize;
        // SAFETY: The native cursor is owned by `self` and writes the field size into `size`.
        ensure_success(
            cursor_call!(self, getSize, index as i32, &mut size),
            "OH_Cursor_GetSize",
        )?;
        Ok(size)
    }

    pub(crate) fn is_null(&self, index: usize) -> Result<bool> {
        let mut is_null = false;
        // SAFETY: The native cursor is owned by `self` and writes the null flag into `is_null`.
        ensure_success(
            cursor_call!(self, isNull, index as i32, &mut is_null),
            "OH_Cursor_IsNull",
        )?;
        Ok(is_null)
    }

    pub(crate) fn int64(&self, index: usize) -> Result<Option<i64>> {
        if self.is_null(index)? {
            return Ok(None);
        }

        let mut value = 0i64;
        // SAFETY: The native cursor is owned by `self` and writes the integer into `value`.
        ensure_success(
            cursor_call!(self, getInt64, index as i32, &mut value),
            "OH_Cursor_GetInt64",
        )?;
        Ok(Some(value))
    }

    pub(crate) fn text(&self, index: usize) -> Result<Option<String>> {
        if self.is_null(index)? {
            return Ok(None);
        }

        let column_type = self.column_type(index)?;
        let size = self.size(index)?;
        if size == 0 {
            return Ok(Some(String::new()));
        }

        if column_type == BLOB_COLUMN_TYPE {
            let mut buffer = vec![0u8; size];
            // SAFETY: The buffer length matches the blob size reported by the cursor.
            ensure_success(
                cursor_call!(
                    self,
                    getBlob,
                    index as i32,
                    buffer.as_mut_ptr() as *mut c_uchar,
                    buffer.len() as c_int
                ),
                "OH_Cursor_GetBlob",
            )?;

            Ok(Some(decode_column(ColumnKind::Blob, size, &buffer)))
        } else {
            let mut buffer = vec![0u8; size + 1];
            // SAFETY: The buffer has one extra byte for the trailing NUL terminator.
            ensure_success(
                cursor_call!(
                    self,
                    getText,
                    index as i32,
                    buffer.as_mut_ptr() as *mut c_char,
                    buffer.len() as c_int
                ),
                "OH_Cursor_GetText",
            )?;

            Ok(Some(decode_column(ColumnKind::Text, size, &buffer)))
        }
    }

    pub(crate) fn blob(&self, index: usize) -> Result<Option<Vec<u8>>> {
        if self.is_null(index)? {
            return Ok(None);
        }

        let size = self.size(index)?;
        if size == 0 {
            return Ok(Some(Vec::new()));
        }

        let mut buffer = vec![0u8; size];
        // SAFETY: The buffer length matches the blob size reported by the cursor.
        ensure_success(
            cursor_call!(
                self,
                getBlob,
                index as i32,
                buffer.as_mut_ptr() as *mut c_uchar,
                buffer.len() as c_int
            ),
            "OH_Cursor_GetBlob",
        )?;

        Ok(Some(buffer))
    }
}

impl Drop for OhosRdbCursor<'_> {
    fn drop(&mut self) {
        let cursor = self.inner.as_ptr();
        // SAFETY: `cursor` is still live here and reading the destroy callback is side-effect free.
        let destroy = unsafe { (*cursor).destroy };
        if let Some(destroy) = destroy {
            // SAFETY: The destroy callback belongs to this live cursor handle.
            unsafe {
                let _ = destroy(cursor);
            }
        }
    }
}
