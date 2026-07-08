//! Servo as an alternative ArkWeb (OpenHarmony) webview backend.
//!
//! This crate builds a single self-contained `cdylib` (`libservo_arkweb.so`) that
//! implements the OHOS `NWebEngine` / `NWeb` C++ interfaces on top of libservo, so that
//! Servo can be selected at runtime as an alternative to the Chromium-based ArkWeb engine
//! behind the same ArkTS `Web` / `WebviewController` API.
//!
//! The port is only meaningful on OpenHarmony targets. The whole crate is gated on
//! `target_env = "ohos"`, so on any other target (e.g. host `./mach check`) it compiles to
//! an empty library.
#![cfg(target_env = "ohos")]

mod bridge;
mod resources;
mod runtime;

// The factory / ABI entry points dlsym'd by the patched nweb_helper.cpp SERVO branch. A Rust
// cdylib localizes symbols that come from linked C++ static archives, so the C++ factory cannot
// be exported directly. Instead these thin `#[unsafe(no_mangle)] pub extern "C"` wrappers (which
// rustc always exports) forward to the C++ `_impl` halves in entry.cpp; referencing the impls
// also forces `entry.o` to be linked in.
use core::ffi::{c_int, c_void};

unsafe extern "C" {
    fn servo_arkweb_create_nweb_engine_impl() -> *mut c_void;
    fn servo_arkweb_abi_version_impl() -> c_int;
}

#[unsafe(no_mangle)]
pub extern "C" fn servo_arkweb_create_nweb_engine() -> *mut c_void {
    unsafe { servo_arkweb_create_nweb_engine_impl() }
}

#[unsafe(no_mangle)]
pub extern "C" fn servo_arkweb_abi_version() -> c_int {
    unsafe { servo_arkweb_abi_version_impl() }
}
