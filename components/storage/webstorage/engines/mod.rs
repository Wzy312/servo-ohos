/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::webstorage::OriginEntry;

#[cfg(ohos_rdb)]
mod ohos_rdb;

#[cfg(feature = "sqlite-backend")]
pub mod sqlite;

#[cfg(all(feature = "sqlite-backend", ohos_rdb))]
mod twin;

// The engine the webstorage manager runs on. A build with both backends
// compiled in picks one per origin, so call sites stay untouched.
#[cfg(all(not(feature = "sqlite-backend"), ohos_rdb))]
pub(crate) use ohos_rdb::OhosRdbEngine as ActiveWebStorageEngine;
#[cfg(all(feature = "sqlite-backend", not(ohos_rdb)))]
#[allow(unused_imports)]
pub(crate) use sqlite::SqliteEngine as ActiveWebStorageEngine;
#[cfg(all(feature = "sqlite-backend", ohos_rdb))]
pub(crate) use twin::TwinEngine as ActiveWebStorageEngine;

pub trait WebStorageEngine {
    type Error;
    fn load(&self) -> Result<OriginEntry, Self::Error>;
    fn clear(&mut self) -> Result<(), Self::Error>;
    fn delete(&mut self, key: &str) -> Result<(), Self::Error>;
    fn set(&mut self, key: &str, value: &str) -> Result<(), Self::Error>;
    fn save(&mut self, data: &OriginEntry);
}
