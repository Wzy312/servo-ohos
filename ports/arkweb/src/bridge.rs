//! The cxx bridge between the C++ NWeb shim and the Rust libservo glue.
//!
//! Two modules, mirroring the intended future diplomat-generated C++ embedder API:
//!   * `servo::embedder` — embedder-generic surface (draft spec for diplomat later).
//!   * `servo::arkweb`   — ArkWeb/OHOS-specific extras.
//!
//! The `extern "Rust"` functions are the C++ -> Rust entry points; their bodies live below
//! and simply delegate to [`crate::runtime`]. The `WebViewClient` opaque C++ type is the
//! engine -> embedder callback sink (implemented by `NWebHandlerProxy` on the C++ side).

use cxx::{CxxString, CxxVector, SharedPtr};

use crate::bridge::ffi::WebViewClient;

#[cxx::bridge(namespace = "servo::embedder")]
pub mod ffi {
    /// Options parsed from the ArkWeb engine init args, passed once at engine startup.
    struct InitOptions {
        user_data_dir: String,
        lang: String,
        /// HTTP(S) proxy URI (from the `web.engine.servo.proxy` system param); empty for none.
        proxy: String,
        extra_args: Vec<String>,
    }

    extern "Rust" {
        fn initialize(options: InitOptions, lazy: bool) -> bool;
        fn shutdown();
        fn create_webview(
            window_handle: usize,
            width: u32,
            height: u32,
            client: SharedPtr<WebViewClient>,
        ) -> u32;
        fn destroy_webview(id: u32);
        fn load_url(id: u32, url: &CxxString);
        fn reload(id: u32);
        fn go_back(id: u32);
        fn go_forward(id: u32);
        /// Traverse the session history by `step` entries (negative = back, positive = forward).
        fn navigate_back_or_forward(id: u32, step: i32);
        fn resize(id: u32, width: u32, height: u32);
        fn set_throttled(id: u32, throttled: bool);
        fn focus(id: u32);
        fn blur(id: u32);
        fn touch_event(id: u32, kind: u8, x: f32, y: f32, pointer_id: i32);
        fn scroll_by(id: u32, dx: f32, dy: f32);
        /// Scroll one viewport up/down; `to_edge` jumps to the start/end of the page instead
        /// (`NWeb::PageUp(top)` / `PageDown(bottom)`).
        fn page_scroll(id: u32, up: bool, to_edge: bool);
        /// Drive the page's active media session: 0 = play, 1 = pause, 2 = stop (constants
        /// mirrored in servo_nweb.cpp).
        fn media_session_action(id: u32, action: i32);
        /// Last known playback state as OHOS `MediaPlaybackState` values
        /// (0 = NONE, 1 = PLAYING, 2 = PAUSED).
        fn get_media_playback_state(id: u32) -> i32;
        fn set_page_zoom(id: u32, zoom: f32);
        fn evaluate_javascript(id: u32, code: &CxxString);
        fn get_url(id: u32) -> String;
        fn get_title(id: u32) -> String;
        fn get_progress(id: u32) -> i32;
        fn can_go_back(id: u32) -> bool;
        fn can_go_forward(id: u32) -> bool;
        /// Whether an editable element is focused and the soft keyboard is up, so ACE performs
        /// keyboard-avoidance layout (`ServoNWeb::NeedSoftKeyboard`).
        fn need_soft_keyboard(id: u32) -> bool;

        // Engine-global cookie access, backed by Servo's site-data manager (sync rendezvous).
        fn cookie_get(url: &CxxString, include_http_only: bool) -> String;
        fn cookie_set(url: &CxxString, value: &CxxString) -> bool;
        fn cookie_clear();
    }

    unsafe extern "C++" {
        include!("servo_client.h");

        /// Abstract engine -> embedder callback sink. `NWebHandlerProxy` subclasses it and
        /// forwards to the ACE-provided `NWebHandler`. Methods take `&self` (const in C++)
        /// because invoking a callback does not mutate the sink itself.
        type WebViewClient;
        fn on_load_started(self: &WebViewClient, url: &CxxString);
        fn on_load_finished(self: &WebViewClient, url: &CxxString, http_status: i32);
        fn on_load_error(self: &WebViewClient, code: i32, desc: &CxxString, url: &CxxString);
        fn on_url_changed(self: &WebViewClient, url: &CxxString);
        fn on_title_changed(self: &WebViewClient, title: &CxxString);
        fn on_progress(self: &WebViewClient, progress: i32);
        fn on_history_changed(self: &WebViewClient, can_back: bool, can_fwd: bool);
        fn on_console_message(
            self: &WebViewClient,
            level: i32,
            msg: &CxxString,
            line: i32,
            source: &CxxString,
        );
        fn on_frame_ready(self: &WebViewClient);
        /// Tell ACE an editable is focused (`true, true`) or blurred (`false, false`) so its back
        /// button closes the soft keyboard and it tracks the focus text field. The keyboard itself
        /// is driven by the engine's own IME connection, not by this.
        fn update_text_field_status(self: &WebViewClient, show_keyboard: bool, attach_ime: bool);
        /// Ask ACE to show a JS dialog (`kind`: 0=alert, 1=confirm, 2=prompt) for the page. Returns
        /// whether a handler took it; the user's response comes back via `resolve_js_dialog`.
        fn show_js_dialog(
            self: &WebViewClient,
            dialog_id: u64,
            kind: i32,
            message: &CxxString,
            default_value: &CxxString,
        ) -> bool;
        /// Ask ACE to show a `<select>` dropdown. `labels` are the flat option labels joined by
        /// `\n`; `selected` is the currently-selected option index (-1 if none). The user's picked
        /// option indices come back via `select_popup_continue`/`select_popup_cancel`.
        fn show_select_popup(
            self: &WebViewClient,
            select_id: u64,
            labels: &CxxString,
            selected: i32,
            multiple: bool,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
        ) -> bool;
        /// Ask ACE to show a `<input type=file>` picker. `accept_types` are the accept filter
        /// patterns joined by `\n` (empty for none); `multiple` allows selecting several files.
        /// Returns whether a handler took it; the chosen paths come back via
        /// `file_picker_continue`/`file_picker_cancel`.
        fn show_file_picker(
            self: &WebViewClient,
            picker_id: u64,
            accept_types: &CxxString,
            multiple: bool,
        ) -> bool;
        /// Ask ACE to show a geolocation permission prompt for `origin`. Returns whether a
        /// handler took it; the decision comes back via `resolve_permission`.
        fn show_geolocation_permission(
            self: &WebViewClient,
            request_id: u64,
            origin: &CxxString,
        ) -> bool;
        /// Ask ACE to show a permission prompt for the `NWebAccessRequest` resource bitmask
        /// (1<<1 video capture, 1<<2 audio capture). The decision comes back via
        /// `resolve_permission`.
        fn show_permission_request(
            self: &WebViewClient,
            request_id: u64,
            origin: &CxxString,
            resources: i32,
        ) -> bool;
        /// Ask ACE to show an HTTP-auth login prompt. Credentials come back via
        /// `resolve_http_auth`.
        fn show_http_auth_request(
            self: &WebViewClient,
            request_id: u64,
            host: &CxxString,
            realm: &CxxString,
        ) -> bool;
    }
}

#[cxx::bridge(namespace = "servo::arkweb")]
pub mod ffi_arkweb {
    extern "Rust" {
        fn key_event(id: u32, keycode: i32, action: i32, unicode: i32) -> bool;
        /// Evaluate JavaScript and deliver the result to the C++ callback registered under
        /// `eval_id` (see `servo_js.h`) once it completes.
        fn evaluate_javascript_with_callback(id: u32, eval_id: u64, code: &CxxString);
        fn init_logging(min_level: i32);
        /// Deliver a JS dialog result from ACE back to the parked `SimpleDialog` (see
        /// `show_js_dialog`). `value` carries the entered text for a confirmed prompt.
        fn resolve_js_dialog(dialog_id: u64, confirmed: bool, value: &CxxString);
        /// Deliver a `<select>` choice from ACE back to the parked `SelectElement`: `indices` are
        /// the picked option indices (flat DOM order).
        fn select_popup_continue(select_id: u64, indices: &CxxVector<i32>);
        /// The `<select>` popup was dismissed without a choice.
        fn select_popup_cancel(select_id: u64);
        /// Deliver the chosen file paths from ACE back to the parked `FilePicker` (see
        /// `show_file_picker`).
        fn file_picker_continue(picker_id: u64, paths: &CxxVector<CxxString>);
        /// The file picker was dismissed without a selection.
        fn file_picker_cancel(picker_id: u64);
        /// Deliver a permission decision from ACE back to the parked `PermissionRequest`
        /// (geolocation and access-request prompts alike, see `show_geolocation_permission` /
        /// `show_permission_request`).
        fn resolve_permission(request_id: u64, allow: bool);
        /// Deliver HTTP-auth credentials (or cancellation) from ACE back to the parked
        /// `AuthenticationRequest` (see `show_http_auth_request`).
        fn resolve_http_auth(
            request_id: u64,
            confirmed: bool,
            username: &CxxString,
            password: &CxxString,
        );
    }

    unsafe extern "C++" {
        include!("servo_native_window.h");

        /// Set the OHNativeWindow buffer geometry before surfman creates/resizes its EGL surface.
        fn set_native_window_buffer_geometry(window: usize, width: u32, height: u32);

        /// Release an OHNativeWindow from CreateNativeWindowFromSurface (call after the rendering
        /// context using it is dropped).
        fn destroy_native_window(window: usize);

        include!("servo_js.h");

        /// Deliver a JavaScript evaluation result to the callback registered under `eval_id`.
        fn deliver_js_result(eval_id: u64, value: &CxxString, success: bool);
    }
}

// ---- `extern "Rust"` implementations (resolved by cxx as `super::<name>`). ----

fn initialize(options: ffi::InitOptions, lazy: bool) -> bool {
    crate::runtime::initialize(options, lazy)
}
fn shutdown() {
    crate::runtime::shutdown()
}
fn create_webview(
    window_handle: usize,
    width: u32,
    height: u32,
    client: SharedPtr<WebViewClient>,
) -> u32 {
    crate::runtime::create_webview(window_handle, width, height, client)
}
fn destroy_webview(id: u32) {
    crate::runtime::destroy_webview(id)
}
fn load_url(id: u32, url: &CxxString) {
    crate::runtime::load_url(id, url)
}
fn reload(id: u32) {
    crate::runtime::reload(id)
}
fn go_back(id: u32) {
    crate::runtime::go_back(id)
}
fn go_forward(id: u32) {
    crate::runtime::go_forward(id)
}
fn navigate_back_or_forward(id: u32, step: i32) {
    crate::runtime::navigate_back_or_forward(id, step)
}
fn resize(id: u32, width: u32, height: u32) {
    crate::runtime::resize(id, width, height)
}
fn set_throttled(id: u32, throttled: bool) {
    crate::runtime::set_throttled(id, throttled)
}
fn focus(id: u32) {
    crate::runtime::focus(id)
}
fn blur(id: u32) {
    crate::runtime::blur(id)
}
fn touch_event(id: u32, kind: u8, x: f32, y: f32, pointer_id: i32) {
    crate::runtime::touch_event(id, kind, x, y, pointer_id)
}
fn scroll_by(id: u32, dx: f32, dy: f32) {
    crate::runtime::scroll_by(id, dx, dy)
}
fn page_scroll(id: u32, up: bool, to_edge: bool) {
    crate::runtime::page_scroll(id, up, to_edge)
}
fn media_session_action(id: u32, action: i32) {
    crate::runtime::media_session_action(id, action)
}
fn get_media_playback_state(id: u32) -> i32 {
    crate::runtime::get_media_playback_state(id)
}
fn set_page_zoom(id: u32, zoom: f32) {
    crate::runtime::set_page_zoom(id, zoom)
}
fn evaluate_javascript(id: u32, code: &CxxString) {
    crate::runtime::evaluate_javascript(id, code)
}
fn get_url(id: u32) -> String {
    crate::runtime::get_url(id)
}
fn get_title(id: u32) -> String {
    crate::runtime::get_title(id)
}
fn get_progress(id: u32) -> i32 {
    crate::runtime::get_progress(id)
}
fn can_go_back(id: u32) -> bool {
    crate::runtime::can_go_back(id)
}
fn can_go_forward(id: u32) -> bool {
    crate::runtime::can_go_forward(id)
}
fn need_soft_keyboard(id: u32) -> bool {
    crate::runtime::need_soft_keyboard(id)
}
fn cookie_get(url: &CxxString, include_http_only: bool) -> String {
    crate::runtime::cookie_get(url, include_http_only)
}
fn cookie_set(url: &CxxString, value: &CxxString) -> bool {
    crate::runtime::cookie_set(url, value)
}
fn cookie_clear() {
    crate::runtime::cookie_clear()
}
fn key_event(id: u32, keycode: i32, action: i32, unicode: i32) -> bool {
    crate::runtime::key_event(id, keycode, action, unicode)
}
fn evaluate_javascript_with_callback(id: u32, eval_id: u64, code: &CxxString) {
    crate::runtime::evaluate_javascript_with_callback(id, eval_id, code)
}
fn init_logging(min_level: i32) {
    crate::runtime::init_logging(min_level)
}
fn resolve_js_dialog(dialog_id: u64, confirmed: bool, value: &CxxString) {
    crate::runtime::resolve_js_dialog(dialog_id, confirmed, value)
}
fn select_popup_continue(select_id: u64, indices: &CxxVector<i32>) {
    crate::runtime::select_popup_continue(select_id, indices)
}
fn select_popup_cancel(select_id: u64) {
    crate::runtime::select_popup_cancel(select_id)
}
fn file_picker_continue(picker_id: u64, paths: &CxxVector<CxxString>) {
    crate::runtime::file_picker_continue(picker_id, paths)
}
fn file_picker_cancel(picker_id: u64) {
    crate::runtime::file_picker_cancel(picker_id)
}
fn resolve_permission(request_id: u64, allow: bool) {
    crate::runtime::resolve_permission(request_id, allow)
}
fn resolve_http_auth(request_id: u64, confirmed: bool, username: &CxxString, password: &CxxString) {
    crate::runtime::resolve_http_auth(request_id, confirmed, username, password)
}
