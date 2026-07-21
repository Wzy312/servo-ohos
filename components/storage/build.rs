/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

fn main() {
    println!("cargo:rustc-check-cfg=cfg(ohos_rdb)");
    println!("cargo:rustc-check-cfg=cfg(ohos_no_backend)");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("ohos") {
        if std::env::var_os("CARGO_FEATURE_OHOS_RDB_BACKEND").is_some() {
            println!("cargo:rustc-cfg=ohos_rdb");
        } else {
            println!("cargo:rustc-cfg=ohos_no_backend");
        }
    }
}
