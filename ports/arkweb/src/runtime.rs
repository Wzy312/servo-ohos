//! The Rust side of the ArkWeb bridge: the servo event-loop thread and per-webview state.
//!
//! Servo and its `WebView`s are `!Send`, so they live entirely on a dedicated "servo-main"
//! thread. Bridge calls arriving on ACE threads are forwarded as [`Action`]s over an mpsc
//! channel; `CreateWebView`/`DestroyWebView` use a response channel to rendezvous. Values the
//! ArkTS side reads synchronously (`getUrl()`, `accessBackward()`, ...) are cached in a shared
//! [`SyncState`] that the [`ArkWebViewDelegate`] updates on the servo thread.
//!
//! Touch, scroll and key input are translated into servo `InputEvent`s here. Not yet wired: the
//! OHOS vsync refresh driver (this uses the default timer-based driver for now).

use std::collections::HashMap;
use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::thread;

use cxx::{CxxString, SharedPtr};
use dpi::PhysicalSize;
use log::{LevelFilter, error, info};
use raw_window_handle::{
    DisplayHandle, OhosDisplayHandle, OhosNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
    WindowHandle,
};
use servo::{
    DevicePoint, DeviceVector2D, EventLoopWaker, InputEvent, Key, KeyState, KeyboardEvent,
    LoadStatus, NamedKey, Opts, RenderingContext, Scroll, Servo, ServoBuilder, TouchEvent,
    TouchEventType, TouchId, TouchPointerType, WebView, WebViewBuilder, WebViewDelegate,
    WindowRenderingContext,
};
use url::Url;

use crate::bridge::ffi::{InitOptions, WebViewClient};

/// A `SharedPtr` to the C++ embedder callback sink, made `Send` so it can travel to the servo
/// thread inside an [`Action`].
struct SendClient(SharedPtr<WebViewClient>);
// SAFETY: the wrapped sink is only ever *invoked* (never mutated concurrently), and ACE's
// NWebHandler implementations are written to be called from the engine thread.
unsafe impl Send for SendClient {}

/// Values the ArkTS side reads synchronously, cached here and updated by [`ArkWebViewDelegate`]
/// on the servo thread.
#[derive(Default)]
struct SyncState {
    url: Mutex<String>,
    title: Mutex<String>,
    progress: AtomicI32,
    can_back: AtomicBool,
    can_fwd: AtomicBool,
}

/// Messages sent from ACE threads to the servo thread. Every variant carries only `Send` data.
enum Action {
    WakeUp,
    CreateWebView {
        id: u32,
        window_handle: usize,
        width: u32,
        height: u32,
        client: SendClient,
        sync: Arc<SyncState>,
        ack: Sender<()>,
    },
    DestroyWebView {
        id: u32,
        ack: Sender<()>,
    },
    LoadUrl {
        id: u32,
        url: String,
    },
    Reload(u32),
    GoBack(u32),
    GoForward(u32),
    Resize {
        id: u32,
        width: u32,
        height: u32,
    },
    SetThrottled {
        id: u32,
        throttled: bool,
    },
    Touch {
        id: u32,
        kind: u8,
        x: f32,
        y: f32,
        pointer_id: i32,
    },
    Scroll {
        id: u32,
        dx: f32,
        dy: f32,
    },
    Focus(u32),
    SetPageZoom {
        id: u32,
        zoom: f32,
    },
    EvaluateJavaScript {
        id: u32,
        code: String,
    },
    Key {
        id: u32,
        key: Key,
        state: KeyState,
    },
}

/// Map an OHOS (ArkUI/MMI) key code plus its unicode value to a servo [`Key`]. Named keys are
/// matched first; any other key with a printable unicode value becomes a `Character`.
fn oh_key_to_servo_key(keycode: i32, unicode: i32) -> Option<Key> {
    let named = match keycode {
        2054 | 2119 => NamedKey::Enter, // KEY_ENTER / KEY_NUMPAD_ENTER
        2055 => NamedKey::Backspace,    // KEY_DEL (deletes backwards)
        2071 => NamedKey::Delete,       // KEY_FORWARD_DEL
        2049 => NamedKey::Tab,          // KEY_TAB
        2070 => NamedKey::Escape,       // KEY_ESCAPE
        2012 => NamedKey::ArrowUp,      // KEY_DPAD_UP
        2013 => NamedKey::ArrowDown,    // KEY_DPAD_DOWN
        2014 => NamedKey::ArrowLeft,    // KEY_DPAD_LEFT
        2015 => NamedKey::ArrowRight,   // KEY_DPAD_RIGHT
        2081 => NamedKey::Home,         // KEY_MOVE_HOME
        2082 => NamedKey::End,          // KEY_MOVE_END
        _ => {
            return u32::try_from(unicode)
                .ok()
                .filter(|&u| u != 0)
                .and_then(char::from_u32)
                .filter(|c| !c.is_control())
                .map(|c| Key::Character(c.to_string()));
        },
    };
    Some(Key::Named(named))
}

static SERVO_CHANNEL: OnceLock<Sender<Action>> = OnceLock::new();
static SYNC_STATES: LazyLock<Mutex<HashMap<u32, Arc<SyncState>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Wrap a bridge-call body so a Rust panic is logged and swallowed rather than unwinding across
/// the C++ boundary (which cxx turns into an abort).
fn guard<T: Default>(name: &str, f: impl FnOnce() -> T) -> T {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => {
            error!("[arkweb] panic caught in bridge fn `{name}`");
            T::default()
        },
    }
}

fn send(action: Action) {
    if let Some(tx) = SERVO_CHANNEL.get() {
        let _ = tx.send(action);
    } else {
        error!("[arkweb] bridge call before InitializeWebEngine");
    }
}

fn with_sync<T: Default>(id: u32, f: impl FnOnce(&Arc<SyncState>) -> T) -> T {
    match SYNC_STATES.lock().unwrap().get(&id) {
        Some(sync) => f(sync),
        None => T::default(),
    }
}

// ---- Servo thread ----

/// Waker that nudges the servo thread to `spin_event_loop`.
#[derive(Clone)]
struct ArkWaker {
    chan: Sender<Action>,
}

impl EventLoopWaker for ArkWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        let _ = self.chan.send(Action::WakeUp);
    }
}

/// The Servo `WebView`, built lazily once a real size is known. The rendering context is held by
/// the painter and the delegate (which paints through it), so it is not kept here separately.
struct BuiltWebView {
    webview: WebView,
}

struct WebViewEntry {
    window_handle: usize,
    client: SharedPtr<WebViewClient>,
    sync: Arc<SyncState>,
    /// The most recent size ACE reported. ACE creates the NWeb at a 1x1 placeholder before layout
    /// and issues one real `Resize` afterwards; surfman must create its EGL surface at the real
    /// size (creating at 1x1 then resizing the producer surface makes WebRender OOM), so the
    /// WebView is not built until this is non-degenerate — mirroring how servoshell only creates a
    /// WebView once a real window size is available.
    size: PhysicalSize<u32>,
    /// A URL requested before the WebView existed / its browsing context was registered. Held here
    /// and flushed by [`ServoThread::flush_pending_loads`] once the WebView is built and ready.
    pending_url: Option<Url>,
    /// `None` until the first real-sized `Resize` builds the WebView.
    built: Option<BuiltWebView>,
}

struct ServoThread {
    servo: Servo,
    webviews: HashMap<u32, WebViewEntry>,
}

impl ServoThread {
    fn run(rx: Receiver<Action>, waker_chan: Sender<Action>, config_dir: PathBuf) {
        // Install the crypto provider Servo's rustls-based networking requires for TLS.
        if rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .is_err()
        {
            info!("[arkweb] rustls crypto provider already installed");
        }
        let waker = Box::new(ArkWaker { chan: waker_chan });
        // `config_dir` must be set: the OHOS font cache unwraps `opts::get().config_dir`
        // (fonts/platform/freetype/ohos/font_cache.rs) when Servo initializes.
        let opts = Opts {
            config_dir: Some(config_dir),
            ..Default::default()
        };
        let servo = ServoBuilder::default()
            .opts(opts)
            .event_loop_waker(waker)
            .build();
        let mut thread = ServoThread {
            servo,
            webviews: HashMap::new(),
        };
        while let Ok(action) = rx.recv() {
            thread.handle(action);
            thread.servo.spin_event_loop();
            thread.flush_pending_loads();
        }
        info!("[arkweb] servo-main thread exiting");
    }

    fn handle(&mut self, action: Action) {
        match action {
            Action::WakeUp => {},
            Action::CreateWebView {
                id,
                window_handle,
                width,
                height,
                client,
                sync,
                ack,
            } => {
                self.create_webview(id, window_handle, width, height, client, sync);
                let _ = ack.send(());
            },
            Action::DestroyWebView { id, ack } => {
                if let Some(entry) = self.webviews.remove(&id) {
                    // Drop the WebView + rendering context first (releases the EGL surface bound to
                    // the native window), then release the native window obtained from
                    // CreateNativeWindowFromSurface, which is otherwise leaked on every close.
                    let window_handle = entry.window_handle;
                    drop(entry);
                    crate::bridge::ffi_arkweb::destroy_native_window(window_handle);
                }
                SYNC_STATES.lock().unwrap().remove(&id);
                let _ = ack.send(());
            },
            Action::LoadUrl { id, url } => match Url::parse(&url) {
                Ok(url) => {
                    if let Some(entry) = self.webviews.get_mut(&id) {
                        // Defer the actual load until the WebView is registered (see below).
                        entry.pending_url = Some(url);
                    }
                },
                Err(error) => error!("[arkweb] load_url id={id}: invalid url {url:?}: {error}"),
            },
            Action::Reload(id) => self.with_webview(id, |wv| wv.reload()),
            Action::GoBack(id) => self.with_webview(id, |wv| {
                wv.go_back(1);
            }),
            Action::GoForward(id) => self.with_webview(id, |wv| {
                wv.go_forward(1);
            }),
            Action::Resize { id, width, height } => {
                info!("[arkweb] resize id={id} {width}x{height}");
                let size = PhysicalSize::new(width.max(1), height.max(1));
                let already_built = match self.webviews.get_mut(&id) {
                    Some(entry) => {
                        entry.size = size;
                        match &entry.built {
                            Some(built) => {
                                crate::bridge::ffi_arkweb::set_native_window_buffer_geometry(
                                    entry.window_handle,
                                    size.width,
                                    size.height,
                                );
                                // `WebView::resize` resizes the shared rendering context itself (via
                                // the painter) *and* issues the WebRender document-view + display-list
                                // update that schedules the repaint. Do not resize the context
                                // directly first: the painter early-returns when the context is
                                // already at the target size, which skips that repaint and leaves the
                                // stale frame until the next unrelated event (e.g. a tap) — most
                                // visibly a half-height page after the soft keyboard closes. Buffer
                                // geometry is still set above, before the surfman resize `resize()`
                                // performs.
                                built.webview.resize(size);
                                true
                            },
                            None => false,
                        }
                    },
                    None => true,
                };
                if !already_built {
                    self.ensure_built(id);
                }
            },
            Action::SetThrottled { id, throttled } => {
                self.with_webview(id, |wv| wv.set_throttled(throttled))
            },
            Action::Touch {
                id,
                kind,
                x,
                y,
                pointer_id,
            } => {
                let event_type = match kind {
                    0 => TouchEventType::Down,
                    1 => TouchEventType::Move,
                    2 => TouchEventType::Up,
                    _ => TouchEventType::Cancel,
                };
                self.with_webview(id, |wv| {
                    wv.notify_input_event(InputEvent::Touch(TouchEvent::new(
                        event_type,
                        TouchId(pointer_id),
                        DevicePoint::new(x, y).into(),
                        TouchPointerType::Touch,
                    )));
                });
            },
            Action::Scroll { id, dx, dy } => {
                // ACE's ScrollBy/ScrollTo carry no anchor; scroll about the viewport centre.
                // Touch-drag scrolling does not use this path — Servo derives it from the touch
                // event sequence itself.
                if let Some(entry) = self.webviews.get(&id) {
                    if let Some(built) = entry.built.as_ref() {
                        let point = DevicePoint::new(
                            entry.size.width as f32 / 2.0,
                            entry.size.height as f32 / 2.0,
                        )
                        .into();
                        built.webview.notify_scroll_event(
                            Scroll::Delta(DeviceVector2D::new(dx, dy).into()),
                            point,
                        );
                    }
                }
            },
            Action::Focus(id) => self.with_webview(id, |wv| wv.focus()),
            Action::SetPageZoom { id, zoom } => self.with_webview(id, |wv| wv.set_page_zoom(zoom)),
            Action::EvaluateJavaScript { id, code } => self.with_webview(id, |wv| {
                // Fire-and-forget for M2; the result callback is surfaced to ACE in M3.
                wv.evaluate_javascript(code, |result| {
                    if let Err(error) = result {
                        error!("[arkweb] evaluate_javascript failed: {error:?}");
                    }
                });
            }),
            Action::Key { id, key, state } => self.with_webview(id, |wv| {
                wv.notify_input_event(InputEvent::Keyboard(KeyboardEvent::from_state_and_key(
                    state, key,
                )));
            }),
        }
    }

    fn with_webview(&self, id: u32, f: impl FnOnce(&WebView)) {
        if let Some(built) = self
            .webviews
            .get(&id)
            .and_then(|entry| entry.built.as_ref())
        {
            f(&built.webview);
        }
    }

    /// Load any URL that was requested before its WebView existed or its browsing context was
    /// registered. A WebView is ready once the constellation has registered it, observable as
    /// `url()` becoming `Some`. Retried after every event-loop turn, so the constellation's own
    /// wakeups drive it.
    fn flush_pending_loads(&mut self) {
        for entry in self.webviews.values_mut() {
            let ready = entry
                .built
                .as_ref()
                .is_some_and(|built| built.webview.url().is_some());
            if ready && entry.pending_url.is_some() {
                let url = entry.pending_url.take().expect("checked is_some");
                info!("[arkweb] flushing pending load: {url}");
                if let Some(built) = &entry.built {
                    built.webview.load(url);
                }
            }
        }
    }

    /// Build the WebView + surfman rendering context once a real (non-1x1) size is known. No-op if
    /// already built or still at the 1x1 placeholder. See [`WebViewEntry::size`].
    fn ensure_built(&mut self, id: u32) {
        let (window_handle, size, client, sync) = match self.webviews.get(&id) {
            Some(entry)
                if entry.built.is_none() && entry.size.width > 1 && entry.size.height > 1 =>
            {
                (
                    entry.window_handle,
                    entry.size,
                    entry.client.clone(),
                    entry.sync.clone(),
                )
            },
            _ => return,
        };

        let Some(native_window) = NonNull::new(window_handle as *mut c_void) else {
            error!("[arkweb] build webview id={id}: null OHNativeWindow");
            return;
        };
        let raw_window = RawWindowHandle::OhosNdk(OhosNdkWindowHandle::new(native_window));
        let raw_display = RawDisplayHandle::Ohos(OhosDisplayHandle::new());
        // SAFETY: the OHNativeWindow / display remain valid until the destroy rendezvous.
        let window = unsafe { WindowHandle::borrow_raw(raw_window) };
        let display = unsafe { DisplayHandle::borrow_raw(raw_display) };

        // surfman does not set the native window's buffer geometry; ACE's producer surface has
        // none, so set it before creating the EGL surface or WebRender OOMs on the mismatch.
        crate::bridge::ffi_arkweb::set_native_window_buffer_geometry(
            window_handle,
            size.width,
            size.height,
        );
        let rendering_context = match WindowRenderingContext::new(display, window, size) {
            Ok(context) => Rc::new(context),
            Err(error) => {
                error!("[arkweb] build webview id={id}: rendering context: {error:?}");
                return;
            },
        };

        let delegate = Rc::new(ArkWebViewDelegate {
            sync,
            client,
            rendering_context: rendering_context.clone(),
        });
        let webview = WebViewBuilder::new(&self.servo, rendering_context.clone())
            .delegate(delegate)
            .build();
        webview.focus();
        webview.show();

        if let Some(entry) = self.webviews.get_mut(&id) {
            entry.built = Some(BuiltWebView { webview });
        }
        info!(
            "[arkweb] built webview id={id} at {}x{}",
            size.width, size.height
        );
    }

    fn create_webview(
        &mut self,
        id: u32,
        window_handle: usize,
        width: u32,
        height: u32,
        client: SendClient,
        sync: Arc<SyncState>,
    ) {
        // Register the WebView's parameters but defer building it: ACE creates the NWeb at a 1x1
        // placeholder before layout, and surfman must create its EGL surface at the real size.
        // The build happens in `ensure_built` on the first non-degenerate `Resize`.
        let size = PhysicalSize::new(width.max(1), height.max(1));
        self.webviews.insert(
            id,
            WebViewEntry {
                window_handle,
                client: client.0,
                sync,
                size,
                pending_url: None,
                built: None,
            },
        );
        info!(
            "[arkweb] register webview id={id} window={window_handle:#x} {width}x{height} (build deferred)"
        );
        // Build immediately if ACE already gave a real size (otherwise wait for the first Resize).
        self.ensure_built(id);
    }
}

/// Per-webview delegate: mirrors Servo notifications into the [`SyncState`] cache and the C++
/// [`WebViewClient`] sink. Invoked on the servo thread (documented MVP constraint).
struct ArkWebViewDelegate {
    sync: Arc<SyncState>,
    client: SharedPtr<WebViewClient>,
    rendering_context: Rc<WindowRenderingContext>,
}

impl WebViewDelegate for ArkWebViewDelegate {
    fn notify_url_changed(&self, _webview: WebView, url: Url) {
        *self.sync.url.lock().unwrap() = url.to_string();
        if let Some(client) = self.client.as_ref() {
            cxx::let_cxx_string!(url = url.as_str());
            client.on_url_changed(&url);
        }
    }

    fn notify_page_title_changed(&self, _webview: WebView, title: Option<String>) {
        let title = title.unwrap_or_default();
        *self.sync.title.lock().unwrap() = title.clone();
        if let Some(client) = self.client.as_ref() {
            cxx::let_cxx_string!(title = &title);
            client.on_title_changed(&title);
        }
    }

    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        let Some(client) = self.client.as_ref() else {
            return;
        };
        let url = webview.url().map(|url| url.to_string()).unwrap_or_default();
        cxx::let_cxx_string!(url = &url);
        match status {
            LoadStatus::Started => client.on_load_started(&url),
            LoadStatus::Complete => client.on_load_finished(&url, 200),
            LoadStatus::HeadParsed => {},
        }
    }

    fn notify_history_changed(&self, _webview: WebView, entries: Vec<Url>, current: usize) {
        let can_back = current > 0;
        let can_fwd = current + 1 < entries.len();
        self.sync.can_back.store(can_back, Ordering::Relaxed);
        self.sync.can_fwd.store(can_fwd, Ordering::Relaxed);
        if let Some(client) = self.client.as_ref() {
            client.on_history_changed(can_back, can_fwd);
        }
    }

    fn notify_new_frame_ready(&self, webview: WebView) {
        // ACE creates the NWeb at a 1x1 placeholder and resizes to the real size once laid out.
        // Presenting into a 1x1 surface makes WebRender report OutOfMemory every frame and panic
        // after five, so skip painting until a real size has arrived.
        let size = self.rendering_context.size();
        if size.width <= 1 || size.height <= 1 {
            return;
        }
        if self.rendering_context.make_current().is_ok() {
            webview.paint();
            self.rendering_context.present();
        }
        if let Some(client) = self.client.as_ref() {
            client.on_frame_ready();
        }
    }
}

// ---- Bridge entry points (called from ACE threads via bridge.rs) ----

pub fn init_logging(min_level: i32) {
    let level = match min_level {
        0 => LevelFilter::Error,
        1 => LevelFilter::Warn,
        2 => LevelFilter::Info,
        3 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };
    let _ = hilog::Builder::new()
        .set_domain(hilog::LogDomain::new(0xE0C3))
        .set_tag("ServoArkWeb")
        .filter_level(level)
        .try_init();
}

pub fn initialize(options: InitOptions) -> bool {
    guard("initialize", || {
        init_logging(2);
        panic::set_hook(Box::new(|info| error!("[arkweb] servo panic: {info}")));
        info!(
            "[arkweb] initialize: user_data_dir={:?} lang={:?} extra_args={:?}",
            options.user_data_dir, options.lang, options.extra_args
        );

        // Servo needs a writable `config_dir` (used for the OHOS font cache and prefs). The OHOS
        // side passes the app's data dir as `--user-data-dir`; fall back to the sandbox-relative
        // app cache dir if it is absent.
        let config_dir = if options.user_data_dir.is_empty() {
            PathBuf::from("/data/storage/el2/base/cache/servo")
        } else {
            PathBuf::from(&options.user_data_dir).join("servo")
        };
        if let Err(error) = std::fs::create_dir_all(&config_dir) {
            error!("[arkweb] failed to create config dir {config_dir:?}: {error}");
        }

        if SERVO_CHANNEL.get().is_some() {
            info!("[arkweb] already initialized");
            return true;
        }
        let (tx, rx) = mpsc::channel::<Action>();
        let waker_chan = tx.clone();
        match thread::Builder::new()
            .name("servo-main".into())
            .spawn(move || ServoThread::run(rx, waker_chan, config_dir))
        {
            Ok(_) => {
                // Publish the channel only once the draining thread is alive. On a spawn failure the
                // engine then stays uninitialized (and `initialize` remains retryable) instead of
                // installing a channel whose receiver never runs and silently swallowing every call.
                let _ = SERVO_CHANNEL.set(tx);
                true
            },
            Err(error) => {
                error!("[arkweb] failed to spawn servo-main thread: {error}");
                false
            },
        }
    })
}

pub fn shutdown() {
    guard("shutdown", || info!("[arkweb] shutdown"))
}

pub fn create_webview(
    window_handle: usize,
    width: u32,
    height: u32,
    client: SharedPtr<WebViewClient>,
) -> u32 {
    guard("create_webview", || {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let sync = Arc::new(SyncState::default());
        SYNC_STATES.lock().unwrap().insert(id, sync.clone());

        let Some(tx) = SERVO_CHANNEL.get() else {
            error!("[arkweb] create_webview before InitializeWebEngine");
            return id;
        };
        let (ack, ack_rx) = mpsc::channel();
        let _ = tx.send(Action::CreateWebView {
            id,
            window_handle,
            width,
            height,
            client: SendClient(client),
            sync,
            ack,
        });
        // Rendezvous: the servo thread only ever blocks in recv(), so this cannot deadlock.
        let _ = ack_rx.recv();
        id
    })
}

pub fn destroy_webview(id: u32) {
    guard("destroy_webview", || {
        let Some(tx) = SERVO_CHANNEL.get() else {
            return;
        };
        let (ack, ack_rx) = mpsc::channel();
        let _ = tx.send(Action::DestroyWebView { id, ack });
        let _ = ack_rx.recv();
        info!("[arkweb] destroy_webview id={id}");
    })
}

pub fn load_url(id: u32, url: &CxxString) {
    guard("load_url", || {
        send(Action::LoadUrl {
            id,
            url: url.to_string_lossy().into_owned(),
        })
    })
}

pub fn reload(id: u32) {
    guard("reload", || send(Action::Reload(id)))
}

pub fn go_back(id: u32) {
    guard("go_back", || send(Action::GoBack(id)))
}

pub fn go_forward(id: u32) {
    guard("go_forward", || send(Action::GoForward(id)))
}

pub fn resize(id: u32, width: u32, height: u32) {
    guard("resize", || send(Action::Resize { id, width, height }))
}

pub fn set_throttled(id: u32, throttled: bool) {
    guard("set_throttled", || {
        send(Action::SetThrottled { id, throttled })
    })
}

pub fn focus(id: u32) {
    guard("focus", || send(Action::Focus(id)))
}

pub fn blur(id: u32) {
    // WebView has no explicit blur; focus is single-webview for the MVP.
    guard("blur", || info!("[arkweb] blur id={id}"))
}

pub fn touch_event(id: u32, kind: u8, x: f32, y: f32, pointer_id: i32) {
    guard("touch_event", || {
        send(Action::Touch {
            id,
            kind,
            x,
            y,
            pointer_id,
        })
    })
}

pub fn scroll_by(id: u32, dx: f32, dy: f32) {
    guard("scroll_by", || send(Action::Scroll { id, dx, dy }))
}

pub fn set_page_zoom(id: u32, zoom: f32) {
    guard("set_page_zoom", || send(Action::SetPageZoom { id, zoom }))
}

pub fn evaluate_javascript(id: u32, code: &CxxString) {
    guard("evaluate_javascript", || {
        send(Action::EvaluateJavaScript {
            id,
            code: code.to_string_lossy().into_owned(),
        })
    })
}

pub fn get_url(id: u32) -> String {
    guard("get_url", || {
        with_sync(id, |sync| sync.url.lock().unwrap().clone())
    })
}

pub fn get_title(id: u32) -> String {
    guard("get_title", || {
        with_sync(id, |sync| sync.title.lock().unwrap().clone())
    })
}

pub fn get_progress(id: u32) -> i32 {
    guard("get_progress", || {
        with_sync(id, |sync| sync.progress.load(Ordering::Relaxed))
    })
}

pub fn can_go_back(id: u32) -> bool {
    guard("can_go_back", || {
        with_sync(id, |sync| sync.can_back.load(Ordering::Relaxed))
    })
}

pub fn can_go_forward(id: u32) -> bool {
    guard("can_go_forward", || {
        with_sync(id, |sync| sync.can_fwd.load(Ordering::Relaxed))
    })
}

/// Translate an OHOS key event into a servo `KeyboardEvent`. `action`: 0 = down, 1 = up (ArkUI
/// `KeyAction`). `unicode` is the character value if any (0 otherwise). Returns whether the key was
/// mapped and dispatched, so ACE can fall back to its own handling for keys we do not consume.
pub fn key_event(id: u32, keycode: i32, action: i32, unicode: i32) -> bool {
    guard("key_event", || {
        let state = match action {
            0 => KeyState::Down,
            1 => KeyState::Up,
            _ => return false,
        };
        let Some(key) = oh_key_to_servo_key(keycode, unicode) else {
            return false;
        };
        // Report the dispatch as unhandled if the engine is not running, so ACE falls back to its
        // own key handling instead of assuming we consumed the event.
        let Some(tx) = SERVO_CHANNEL.get() else {
            error!("[arkweb] key_event before InitializeWebEngine");
            return false;
        };
        tx.send(Action::Key { id, key, state }).is_ok()
    })
}
