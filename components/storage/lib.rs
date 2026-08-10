/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#[cfg(not(any(feature = "sqlite-backend", ohos_rdb)))]
compile_error!(
    "no storage backend selected: enable `sqlite-backend` or (on OHOS) `ohos-rdb-backend`"
);

#[cfg(ohos_rdb)]
pub(crate) mod blob_text;
#[cfg(ohos_rdb)]
pub(crate) mod client_storage_shared;

#[cfg(ohos_rdb)]
mod ohos_rdb;

pub mod cache_storage;
pub mod client_storage;
mod indexeddb;
pub(crate) mod shared;
mod storage_thread;
mod webstorage;

pub use cache_storage::CacheStorageThreadFactory;
pub use client_storage::ClientStorageThreadFactory;
pub(crate) use indexeddb::IndexedDBThreadFactory;
pub use storage_thread::new_storage_threads;
pub(crate) use webstorage::WebStorageThreadFactory;
