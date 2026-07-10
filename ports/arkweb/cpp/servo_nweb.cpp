#include "servo_nweb.h"

#include <utility>

#include "arkweb/src/bridge.rs.h"
#include "ohos_nweb/nweb_web_message.h"
#include "servo_js.h"
#include "servo_stub_managers.h"

namespace OHOS::NWeb {

namespace {
// Touch kinds, matching the `kind` argument of the `touch_event` bridge function.
constexpr std::uint8_t kTouchDown = 0;
constexpr std::uint8_t kTouchMove = 1;
constexpr std::uint8_t kTouchUp = 2;
constexpr std::uint8_t kTouchCancel = 3;

// Media-session actions, matching the `action` argument of the `media_session_action` bridge
// function.
constexpr std::int32_t kMediaActionPlay = 0;
constexpr std::int32_t kMediaActionPause = 1;
constexpr std::int32_t kMediaActionStop = 2;

// ACE stores and dereferences the returned drag data (web_delegate.cpp GetOrCreateDragData), so
// GetOrCreateDragData must return non-null even though Servo exposes no drag payload yet.
class ServoDragData : public NWebDragData {
public:
    std::string GetLinkURL() override { return {}; }
    std::string GetFragmentText() override { return {}; }
    std::string GetFragmentHtml() override { return {}; }
    bool GetPixelMapSetting(const void** /*data*/, size_t& /*len*/, int& /*width*/,
                            int& /*height*/) override {
        return false;
    }
    bool SetFragmentHtml(const std::string& /*html*/) override { return false; }
    bool SetPixelMapSetting(const void* /*data*/, size_t /*len*/, int /*width*/,
                            int /*height*/) override {
        return false;
    }
    bool SetLinkURL(const std::string& /*url*/) override { return false; }
    bool SetFragmentText(const std::string& /*text*/) override { return false; }
    std::string GetLinkTitle() override { return {}; }
    bool SetLinkTitle(const std::string& /*title*/) override { return false; }
    void GetDragStartPosition(int& x, int& y) override {
        x = 0;
        y = 0;
    }
    bool IsSingleImageContent() override { return false; }
    bool SetFileUri(const std::string& /*uri*/) override { return false; }
    std::string GetImageFileName() override { return {}; }
    void ClearImageFileNames() override {}
};
}  // namespace

ServoNWeb::ServoNWeb(uint32_t id, std::shared_ptr<servo::arkweb::NWebHandlerProxy> proxy)
    : id_(id), proxy_(std::move(proxy)) {}

void ServoNWeb::Resize(uint32_t width, uint32_t height, bool /*isKeyboard*/) {
    servo::embedder::resize(id_, width, height);
}

void ServoNWeb::OnPause() {
    paused_ = true;
    UpdateThrottled();
}

void ServoNWeb::OnContinue() {
    paused_ = false;
    UpdateThrottled();
}

// The RS surface-occlusion callback is not on the OnPause path, so without these Servo keeps
// rendering at full rate while fully covered by another window.
void ServoNWeb::OnOccluded() {
    occluded_ = true;
    UpdateThrottled();
}

void ServoNWeb::OnUnoccluded() {
    occluded_ = false;
    UpdateThrottled();
}

void ServoNWeb::UpdateThrottled() {
    servo::embedder::set_throttled(id_, paused_ || occluded_);
}

void ServoNWeb::OnDestroy() {
    if (native_destroy_callback_ != nullptr) {
        native_destroy_callback_(native_destroy_web_name_.c_str());
    }
    servo::embedder::destroy_webview(id_);
}

void ServoNWeb::OnFocus(const FocusReason& /*focusReason*/) {
    servo::embedder::focus(id_);
}

void ServoNWeb::OnBlur(const BlurReason& /*blurReason*/) {
    servo::embedder::blur(id_);
}

void ServoNWeb::OnTouchPress(int32_t id, double x, double y, bool /*fromOverlay*/) {
    servo::embedder::touch_event(id_, kTouchDown, static_cast<float>(x), static_cast<float>(y), id);
}

void ServoNWeb::OnTouchRelease(int32_t id, double x, double y, bool /*fromOverlay*/) {
    servo::embedder::touch_event(id_, kTouchUp, static_cast<float>(x), static_cast<float>(y), id);
}

void ServoNWeb::OnTouchMove(int32_t id, double x, double y, bool /*fromOverlay*/) {
    servo::embedder::touch_event(id_, kTouchMove, static_cast<float>(x), static_cast<float>(y), id);
}

// ACE forwards touch-move during a pan through this batched overload (web_delegate.cpp
// HandleTouchMove), not the single-point one, so it must be overridden or drags are dropped.
void ServoNWeb::OnTouchMove(const std::vector<std::shared_ptr<NWebTouchPointInfo>>& touch_point_infos,
                            bool /*fromOverlay*/) {
    for (const auto& point : touch_point_infos) {
        if (point) {
            servo::embedder::touch_event(id_, kTouchMove, static_cast<float>(point->GetX()),
                                         static_cast<float>(point->GetY()), point->GetId());
        }
    }
}

void ServoNWeb::OnTouchCancel() {
    servo::embedder::touch_event(id_, kTouchCancel, 0.0F, 0.0F, -1);
}

bool ServoNWeb::SendKeyEvent(int32_t keyCode, int32_t keyAction) {
    return servo::arkweb::key_event(id_, keyCode, keyAction, 0);
}

// ACE routes physical key input through here (web_pattern.cpp WebOnKeyEvent), not SendKeyEvent.
// NWebKeyboardEvent additionally carries the resolved unicode value, used for text characters.
bool ServoNWeb::SendKeyboardEvent(const std::shared_ptr<NWebKeyboardEvent>& keyboardEvent) {
    if (!keyboardEvent) {
        return false;
    }
    return servo::arkweb::key_event(id_, keyboardEvent->GetKeyCode(), keyboardEvent->GetAction(),
                                    keyboardEvent->GetUnicode());
}

// Servo owns the system IME connection itself (via the OHOS InputMethod NDK). ACE still queries
// this to decide whether to perform keyboard-avoidance layout; report true while Servo has an
// editable focused and the soft keyboard up.
bool ServoNWeb::NeedSoftKeyboard() {
    return servo::embedder::need_soft_keyboard(id_);
}

int ServoNWeb::Load(const std::string& url) {
    servo::embedder::load_url(id_, url);
    return 0;
}

bool ServoNWeb::IsNavigatebackwardAllowed() {
    return servo::embedder::can_go_back(id_);
}

bool ServoNWeb::IsNavigateForwardAllowed() {
    return servo::embedder::can_go_forward(id_);
}

bool ServoNWeb::CanNavigateBackOrForward(int numSteps) {
    return numSteps < 0 ? servo::embedder::can_go_back(id_) : servo::embedder::can_go_forward(id_);
}

void ServoNWeb::NavigateBack() {
    servo::embedder::go_back(id_);
}

void ServoNWeb::NavigateForward() {
    servo::embedder::go_forward(id_);
}

void ServoNWeb::NavigateBackOrForward(int step) {
    servo::embedder::navigate_back_or_forward(id_, step);
}

void ServoNWeb::Reload() {
    servo::embedder::reload(id_);
}

int ServoNWeb::Zoom(float zoomFactor) {
    servo::embedder::set_page_zoom(id_, zoomFactor);
    return 0;
}

void ServoNWeb::ExecuteJavaScript(const std::string& code) {
    servo::embedder::evaluate_javascript(id_, code);
}

// The result-returning form ArkTS `runJavaScript(script, callback)` routes through. The callback
// is parked by id and invoked from the servo thread once evaluation completes (servo_js.cpp).
void ServoNWeb::ExecuteJavaScript(const std::string& code,
                                  std::shared_ptr<NWebMessageValueCallback> callback,
                                  bool /*extention*/) {
    std::uint64_t eval_id = servo::arkweb::register_js_callback(std::move(callback));
    servo::arkweb::evaluate_javascript_with_callback(id_, eval_id, code);
}

unsigned int ServoNWeb::GetWebId() {
    return id_;
}

void ServoNWeb::SetNWebHandler(std::shared_ptr<NWebHandler> handler) {
    if (proxy_) {
        proxy_->set_handler(std::move(handler));
    }
}

std::string ServoNWeb::GetUrl() {
    auto url = servo::embedder::get_url(id_);
    return std::string(url.data(), url.size());
}

std::string ServoNWeb::Title() {
    auto title = servo::embedder::get_title(id_);
    return std::string(title.data(), title.size());
}

int ServoNWeb::PageLoadProgress() {
    return servo::embedder::get_progress(id_);
}

// Servo tracks no pre-redirect URL; the committed URL is the closest available answer.
const std::string ServoNWeb::GetOriginalUrl() {
    auto url = servo::embedder::get_url(id_);
    return std::string(url.data(), url.size());
}

// ScrollTo is absolute; the bridge currently exposes only relative scrolling. Approximated
// for the MVP (proper absolute scroll tracked for a later milestone).
void ServoNWeb::ScrollTo(float x, float y) {
    servo::embedder::scroll_by(id_, x, y);
}

void ServoNWeb::ScrollBy(float delta_x, float delta_y) {
    servo::embedder::scroll_by(id_, delta_x, delta_y);
}

// Nested-scroll handoff from an enclosing ArkUI scrollable. The fling velocity is dropped:
// Servo exposes no velocity-scroll injection, and the delta alone keeps content tracking.
void ServoNWeb::ScrollByRefScreen(float delta_x, float delta_y, float /*vx*/, float /*vy*/) {
    servo::embedder::scroll_by(id_, delta_x, delta_y);
}

void ServoNWeb::PageUp(bool top) {
    servo::embedder::page_scroll(id_, true, top);
}

void ServoNWeb::PageDown(bool bottom) {
    servo::embedder::page_scroll(id_, false, bottom);
}

// These reach the page's *active media session* only; a page that never touches the
// MediaSession API may not respond (documented Tier-1 limitation).
void ServoNWeb::PauseAllMedia() {
    servo::embedder::media_session_action(id_, kMediaActionPause);
}

void ServoNWeb::ResumeAllMedia() {
    servo::embedder::media_session_action(id_, kMediaActionPlay);
}

void ServoNWeb::StopAllMedia() {
    servo::embedder::media_session_action(id_, kMediaActionStop);
}

int ServoNWeb::GetMediaPlaybackState() {
    return servo::embedder::get_media_playback_state(id_);
}

void ServoNWeb::PutBackgroundColor(int color) {
    background_color_ = color;
}

std::shared_ptr<NWebPreference> ServoNWeb::GetPreference() {
    return GetServoPreference();
}

void ServoNWeb::SetNWebJavaScriptResultCallBack(
    std::shared_ptr<NWebJavaScriptResultCallBack> callback) {
    js_result_callback_ = std::move(callback);
}

void ServoNWeb::RegisterNativeValideCallback(const char* webName,
                                             const NativeArkWebOnValidCallback callback) {
    if (callback != nullptr && webName != nullptr) {
        callback(webName);
    }
}

void ServoNWeb::RegisterNativeDestroyCallback(const char* webName,
                                              const NativeArkWebOnDestroyCallback callback) {
    native_destroy_web_name_ = webName != nullptr ? webName : "";
    native_destroy_callback_ = callback;
}

std::shared_ptr<NWebDragData> ServoNWeb::GetOrCreateDragData() {
    if (!drag_data_) {
        drag_data_ = std::make_shared<ServoDragData>();
    }
    return drag_data_;
}

// Servo has no JS precompile/code-cache API. The NAPI callback unconditionally reads GetInt64()
// off the message and rejects the ArkTS promise for any non-OK (non-zero) code, so deliver an
// INTEGER message immediately rather than leaving the promise hanging.
void ServoNWeb::PrecompileJavaScript(const std::string& /*url*/, const std::string& /*script*/,
                                     std::shared_ptr<CacheOptions>& /*cacheOptions*/,
                                     std::shared_ptr<NWebMessageValueCallback> callback) {
    if (!callback) {
        return;
    }
    auto message = std::make_shared<NWebMessage>(NWebValue::Type::INTEGER);
    message->SetInt64(-1);
    callback->OnReceiveValue(std::move(message));
}

}  // namespace OHOS::NWeb
