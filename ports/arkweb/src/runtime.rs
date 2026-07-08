//! The Rust side of the ArkWeb bridge: the servo event-loop thread and per-webview state.
//!
//! Servo and its `WebView`s are `!Send`, so they live entirely on a dedicated "servo-main"
//! thread. Bridge calls arriving on ACE threads are forwarded as [`Action`]s over an mpsc
//! channel; `CreateWebView`/`DestroyWebView` use a response channel to rendezvous. Values the
//! ArkTS side reads synchronously (`getUrl()`, `accessBackward()`, ...) are cached in a shared
//! [`SyncState`] that the [`ArkWebViewDelegate`] updates on the servo thread.
//!
//! Not yet wired (tracked as `TODO(arkweb)`): input injection (touch/key/scroll — M2) and the
//! OHOS vsync refresh driver (this uses the default timer-based driver for now).

use std::collections::HashMap;
use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
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
    EventLoopWaker, LoadStatus, RenderingContext, Servo, ServoBuilder, WebView, WebViewBuilder,
    WebViewDelegate, WindowRenderingContext,
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
    Focus(u32),
    SetPageZoom {
        id: u32,
        zoom: f32,
    },
    EvaluateJavaScript {
        id: u32,
        code: String,
    },
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

struct WebViewEntry {
    webview: WebView,
    rendering_context: Rc<WindowRenderingContext>,
}

struct ServoThread {
    servo: Servo,
    webviews: HashMap<u32, WebViewEntry>,
}

impl ServoThread {
    fn run(rx: Receiver<Action>, waker_chan: Sender<Action>) {
        // Install the crypto provider Servo's rustls-based networking requires for TLS.
        if rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .is_err()
        {
            info!("[arkweb] rustls crypto provider already installed");
        }
        let waker = Box::new(ArkWaker { chan: waker_chan });
        let servo = ServoBuilder::default().event_loop_waker(waker).build();
        let mut thread = ServoThread {
            servo,
            webviews: HashMap::new(),
        };
        while let Ok(action) = rx.recv() {
            thread.handle(action);
            thread.servo.spin_event_loop();
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
                self.webviews.remove(&id);
                SYNC_STATES.lock().unwrap().remove(&id);
                let _ = ack.send(());
            },
            Action::LoadUrl { id, url } => {
                if let (Some(entry), Ok(url)) = (self.webviews.get(&id), Url::parse(&url)) {
                    entry.webview.load(url);
                }
            },
            Action::Reload(id) => self.with_webview(id, |wv| wv.reload()),
            Action::GoBack(id) => self.with_webview(id, |wv| {
                wv.go_back(1);
            }),
            Action::GoForward(id) => self.with_webview(id, |wv| {
                wv.go_forward(1);
            }),
            Action::Resize { id, width, height } => {
                if let Some(entry) = self.webviews.get(&id) {
                    let size = PhysicalSize::new(width.max(1), height.max(1));
                    entry.rendering_context.resize(size);
                    entry.webview.resize(size);
                }
            },
            Action::SetThrottled { id, throttled } => {
                self.with_webview(id, |wv| wv.set_throttled(throttled))
            },
            Action::Focus(id) => self.with_webview(id, |wv| wv.focus()),
            Action::SetPageZoom { id, zoom } => self.with_webview(id, |wv| wv.set_page_zoom(zoom)),
            Action::EvaluateJavaScript { id, code } => {
                // TODO(arkweb): WebView JS evaluation + result callback (M3).
                let _ = (id, code);
            },
        }
    }

    fn with_webview(&self, id: u32, f: impl FnOnce(&WebView)) {
        if let Some(entry) = self.webviews.get(&id) {
            f(&entry.webview);
        }
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
        let Some(native_window) = NonNull::new(window_handle as *mut c_void) else {
            error!("[arkweb] create_webview id={id}: null OHNativeWindow");
            return;
        };
        let raw_window = RawWindowHandle::OhosNdk(OhosNdkWindowHandle::new(native_window));
        let raw_display = RawDisplayHandle::Ohos(OhosDisplayHandle::new());
        // SAFETY: the OHNativeWindow / display remain valid until the destroy rendezvous.
        let window = unsafe { WindowHandle::borrow_raw(raw_window) };
        let display = unsafe { DisplayHandle::borrow_raw(raw_display) };

        let size = PhysicalSize::new(width.max(1), height.max(1));
        let rendering_context = match WindowRenderingContext::new(display, window, size) {
            Ok(context) => Rc::new(context),
            Err(error) => {
                error!("[arkweb] create_webview id={id}: rendering context: {error:?}");
                return;
            },
        };

        let delegate = Rc::new(ArkWebViewDelegate {
            sync,
            client: client.0,
            rendering_context: rendering_context.clone(),
        });
        let webview = WebViewBuilder::new(&self.servo, rendering_context.clone())
            .delegate(delegate)
            .build();
        webview.focus();
        webview.show();

        self.webviews.insert(
            id,
            WebViewEntry {
                webview,
                rendering_context,
            },
        );
        info!("[arkweb] create_webview id={id} window={window_handle:#x} {width}x{height}");
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

        let (tx, rx) = mpsc::channel::<Action>();
        if SERVO_CHANNEL.set(tx.clone()).is_err() {
            info!("[arkweb] already initialized");
            return true;
        }
        let waker_chan = tx;
        match thread::Builder::new()
            .name("servo-main".into())
            .spawn(move || ServoThread::run(rx, waker_chan))
        {
            Ok(_) => true,
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
    // TODO(arkweb): translate to servo InputEvent/TouchEvent (M2).
    guard("touch_event", || {
        info!("[arkweb] touch id={id} kind={kind} ({x},{y}) pointer={pointer_id}")
    })
}

pub fn scroll_by(id: u32, dx: f32, dy: f32) {
    // TODO(arkweb): translate to a servo scroll/wheel InputEvent (M2).
    guard("scroll_by", || {
        info!("[arkweb] scroll_by id={id} ({dx},{dy})")
    })
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

pub fn send_key_event(id: u32, oh_keycode: i32, oh_action: i32) -> bool {
    // TODO(arkweb): translate to a servo KeyboardEvent (M2).
    guard("send_key_event", || {
        info!("[arkweb] key id={id} code={oh_keycode} action={oh_action}");
        false
    })
}
