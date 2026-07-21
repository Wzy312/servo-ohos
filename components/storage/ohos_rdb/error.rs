/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(ohos_rdb)]

use std::ffi::NulError;
use std::fmt;
use std::io;
use std::os::raw::c_int;
use std::ptr::NonNull;

use ohos_rdb_sys::relational_store_error_code::OH_Rdb_ErrCode;

#[derive(Debug)]
pub(crate) enum OhosRdbError {
    NullHandle(&'static str),
    MissingMethod(&'static str),
    Api { context: &'static str, code: c_int },
    Nul(NulError),
    Io(io::Error),
    Postcard(postcard::Error),
}

pub(crate) type Result<T> = std::result::Result<T, OhosRdbError>;

pub(crate) fn ensure_success(status: i32, context: &'static str) -> Result<()> {
    if status == OH_Rdb_ErrCode::RDB_OK.0 {
        Ok(())
    } else {
        Err(OhosRdbError::Api {
            context,
            code: status,
        })
    }
}

pub(crate) fn into_handle<T>(raw: *mut T, context: &'static str) -> Result<NonNull<T>> {
    NonNull::new(raw).ok_or(OhosRdbError::NullHandle(context))
}

pub(crate) fn missing_method(name: &'static str) -> OhosRdbError {
    OhosRdbError::MissingMethod(name)
}

impl fmt::Display for OhosRdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullHandle(context) => write!(f, "{context} returned a null handle"),
            Self::MissingMethod(name) => write!(f, "missing OHOS RDB method pointer: {name}"),
            Self::Api { context, code } => write!(f, "{context} failed with status {code}"),
            Self::Nul(err) => err.fmt(f),
            Self::Io(err) => err.fmt(f),
            Self::Postcard(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for OhosRdbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Nul(err) => Some(err),
            Self::Io(err) => Some(err),
            Self::Postcard(err) => Some(err),
            _ => None,
        }
    }
}

impl From<NulError> for OhosRdbError {
    fn from(err: NulError) -> Self {
        Self::Nul(err)
    }
}

impl From<io::Error> for OhosRdbError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<postcard::Error> for OhosRdbError {
    fn from(err: postcard::Error) -> Self {
        Self::Postcard(err)
    }
}
