/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#[allow(dead_code)]
#[path = "../blob_text.rs"]
mod blob_text;
mod cache_storage;
mod client_storage;
#[allow(dead_code)]
#[path = "../client_storage_shared.rs"]
mod client_storage_shared;
mod indexeddb;
mod storage_thread;
mod webstorage;
