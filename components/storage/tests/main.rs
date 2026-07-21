/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![cfg(not(target_env = "ohos"))]

mod client_storage;
#[path = "../blob_text.rs"]
mod blob_text;
#[path = "../client_storage_shared.rs"]
mod client_storage_shared;
mod indexeddb;
mod storage_thread;
mod webstorage;
