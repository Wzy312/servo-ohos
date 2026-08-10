/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(ohos_rdb)]

#[cfg(test)]
use std::ffi::CStr;
use std::ffi::CString;
#[cfg(test)]
use std::os::raw::c_char;
use std::ptr::NonNull;

use ohos_rdb_sys::data_values::{
    OH_Values_Count, OH_Values_Create, OH_Values_Destroy, OH_Values_PutBlob, OH_Values_PutInt,
    OH_Values_PutNull, OH_Values_PutText,
};
#[cfg(test)]
use ohos_rdb_sys::data_values::{
    OH_Values_GetBlob, OH_Values_GetInt, OH_Values_GetText, OH_Values_GetType, OH_Values_IsNull,
};
#[cfg(test)]
use ohos_rdb_sys::rdb_types::OH_ColumnType;
use ohos_rdb_sys::rdb_types::OH_Data_Values;

use super::error::{Result, ensure_success, into_handle};

#[derive(Debug)]
pub(crate) struct OhosRdbValues {
    inner: NonNull<OH_Data_Values>,
}

#[cfg(test)]
const BLOB_COLUMN_TYPE: OH_ColumnType = OH_ColumnType(4);

impl OhosRdbValues {
    pub(crate) fn new() -> Result<Self> {
        // SAFETY: The constructor returns an owned handle that `from_raw` validates.
        unsafe { Self::from_raw(OH_Values_Create()) }
    }

    /// # Safety
    ///
    /// `raw` must be a live `OH_Data_Values` handle returned by `OH_Values_Create`.
    /// This wrapper takes ownership and destroys it in `Drop`.
    pub(crate) unsafe fn from_raw(raw: *mut OH_Data_Values) -> Result<Self> {
        Ok(Self {
            inner: into_handle(raw, "OH_Values_Create")?,
        })
    }

    pub(crate) fn as_ptr(&self) -> *const OH_Data_Values {
        self.inner.as_ptr() as *const OH_Data_Values
    }

    fn as_mut_ptr(&self) -> *mut OH_Data_Values {
        self.inner.as_ptr()
    }

    pub(crate) fn push_null(&mut self) -> Result<()> {
        // SAFETY: The values handle is owned by `self` and the API writes no extra memory.
        unsafe { ensure_success(OH_Values_PutNull(self.as_mut_ptr()), "OH_Values_PutNull") }
    }

    pub(crate) fn push_int(&mut self, value: i64) -> Result<()> {
        // SAFETY: The values handle is owned by `self` and the integer is passed by value.
        unsafe {
            ensure_success(
                OH_Values_PutInt(self.as_mut_ptr(), value),
                "OH_Values_PutInt",
            )
        }
    }

    pub(crate) fn push_text(&mut self, value: &str) -> Result<()> {
        // SAFETY: The values handle is owned by `self`; text and blob payloads borrow `value`.
        unsafe {
            if value.as_bytes().contains(&0) {
                ensure_success(
                    OH_Values_PutBlob(self.as_mut_ptr(), value.as_ptr(), value.len()),
                    "OH_Values_PutBlob",
                )
            } else {
                let value = CString::new(value)?;
                ensure_success(
                    OH_Values_PutText(self.as_mut_ptr(), value.as_ptr()),
                    "OH_Values_PutText",
                )
            }
        }
    }

    pub(crate) fn push_blob(&mut self, value: &[u8]) -> Result<()> {
        // SAFETY: The values handle is owned by `self` and the blob borrow stays live for the call.
        unsafe {
            ensure_success(
                OH_Values_PutBlob(self.as_mut_ptr(), value.as_ptr(), value.len()),
                "OH_Values_PutBlob",
            )
        }
    }

    pub(crate) fn count(&self) -> Result<usize> {
        let mut count = 0usize;
        // SAFETY: The values handle is owned by `self` and writes the count into `count`.
        unsafe {
            ensure_success(
                OH_Values_Count(self.as_mut_ptr(), &mut count),
                "OH_Values_Count",
            )?;
        }
        Ok(count)
    }

    #[cfg(test)]
    pub(crate) fn is_null(&self, index: usize) -> Result<bool> {
        let mut is_null = false;
        // SAFETY: The values handle is owned by `self` and writes the null flag into `is_null`.
        unsafe {
            ensure_success(
                OH_Values_IsNull(self.as_mut_ptr(), index as i32, &mut is_null),
                "OH_Values_IsNull",
            )?;
        }
        Ok(is_null)
    }

    #[cfg(test)]
    pub(crate) fn int(&self, index: usize) -> Result<Option<i64>> {
        if self.is_null(index)? {
            return Ok(None);
        }

        let mut value = 0i64;
        // SAFETY: The values handle is owned by `self` and writes the integer into `value`.
        unsafe {
            ensure_success(
                OH_Values_GetInt(self.as_mut_ptr(), index as i32, &mut value),
                "OH_Values_GetInt",
            )?;
        }
        Ok(Some(value))
    }

    #[cfg(test)]
    pub(crate) fn text(&self, index: usize) -> Result<Option<String>> {
        if self.is_null(index)? {
            return Ok(None);
        }

        let data_type = self.column_type(index)?;
        if data_type == BLOB_COLUMN_TYPE {
            let mut value: *const u8 = std::ptr::null();
            let mut length = 0usize;
            // SAFETY: The values handle is owned by `self` and writes a borrowed blob pointer/length.
            unsafe {
                ensure_success(
                    OH_Values_GetBlob(self.as_mut_ptr(), index as i32, &mut value, &mut length),
                    "OH_Values_GetBlob",
                )?;
            }

            if length == 0 {
                return Ok(Some(String::new()));
            }

            let value = NonNull::new(value as *mut u8)
                .ok_or(super::OhosRdbError::NullHandle("OH_Values_GetBlob"))?;
            let value = unsafe { std::slice::from_raw_parts(value.as_ptr(), length) };
            return Ok(Some(String::from_utf8_lossy(value).into_owned()));
        }

        let mut value: *const c_char = std::ptr::null();
        // SAFETY: The values handle is owned by `self` and writes a borrowed text pointer.
        unsafe {
            ensure_success(
                OH_Values_GetText(self.as_mut_ptr(), index as i32, &mut value),
                "OH_Values_GetText",
            )?;
        }

        let value = NonNull::new(value as *mut c_char)
            .ok_or(super::OhosRdbError::NullHandle("OH_Values_GetText"))?;
        let value = unsafe { CStr::from_ptr(value.as_ptr()) };
        Ok(Some(value.to_string_lossy().into_owned()))
    }

    #[cfg(test)]
    fn column_type(&self, index: usize) -> Result<OH_ColumnType> {
        let mut value = OH_ColumnType(0);
        // SAFETY: The values handle is owned by `self` and writes the column type into `value`.
        unsafe {
            ensure_success(
                OH_Values_GetType(self.as_mut_ptr(), index as i32, &mut value),
                "OH_Values_GetType",
            )?;
        }
        Ok(value)
    }
}

impl Drop for OhosRdbValues {
    fn drop(&mut self) {
        // SAFETY: The handle belongs to `self` and is destroyed exactly once here.
        unsafe {
            let _ = OH_Values_Destroy(self.inner.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use ohos_rdb_sys::data_values::OH_Values_GetType;
    use ohos_rdb_sys::rdb_types::OH_ColumnType;

    use super::*;

    #[test]
    fn binds_text_int_and_null_values_preserve_interior_nul() {
        let mut values = OhosRdbValues::new().unwrap();

        values.push_text("alpha").unwrap();
        values.push_text("a\0b").unwrap();
        values.push_int(7).unwrap();
        values.push_null().unwrap();

        assert_eq!(values.count().unwrap(), 4);
        assert_eq!(values.text(0).unwrap(), Some(String::from("alpha")));
        assert_eq!(values.text(1).unwrap(), Some(String::from("a\0b")));
        assert_eq!(values.int(2).unwrap(), Some(7));
        assert!(values.is_null(3).unwrap());

        let mut data_type = OH_ColumnType(0);
        unsafe {
            ensure_success(
                OH_Values_GetType(values.as_mut_ptr(), 0, &mut data_type),
                "OH_Values_GetType",
            )
            .unwrap();
        }
        assert_eq!(data_type, OH_ColumnType(3));

        unsafe {
            ensure_success(
                OH_Values_GetType(values.as_mut_ptr(), 1, &mut data_type),
                "OH_Values_GetType",
            )
            .unwrap();
        }
        assert_eq!(data_type, OH_ColumnType(4));
    }
}
