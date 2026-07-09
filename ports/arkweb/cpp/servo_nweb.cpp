#include "servo_nweb.h"

#include <utility>

#include "arkweb/src/bridge.rs.h"
#include "servo_js.h"
#include "servo_stub_managers.h"

namespace OHOS::NWeb {

namespace {
// Touch kinds, matching the `kind` argument of the `touch_event` bridge function.
constexpr std::uint8_t kTouchDown = 0;
constexpr std::uint8_t kTouchMove = 1;
constexpr std::uint8_t kTouchUp = 2;
constexpr std::uint8_t kTouchCancel = 3;
}  // namespace

ServoNWeb::ServoNWeb(uint32_t id, std::shared_ptr<servo::arkweb::NWebHandlerProxy> proxy)
    : id_(id), proxy_(std::move(proxy)) {}

void ServoNWeb::Resize(uint32_t width, uint32_t height, bool /*isKeyboard*/) {
    servo::embedder::resize(id_, width, height);
}

void ServoNWeb::OnPause() {
    servo::embedder::set_throttled(id_, true);
}

void ServoNWeb::OnContinue() {
    servo::embedder::set_throttled(id_, false);
}

void ServoNWeb::OnDestroy() {
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

// ScrollTo is absolute; the bridge currently exposes only relative scrolling. Approximated
// for the MVP (proper absolute scroll tracked for a later milestone).
void ServoNWeb::ScrollTo(float x, float y) {
    servo::embedder::scroll_by(id_, x, y);
}

void ServoNWeb::ScrollBy(float delta_x, float delta_y) {
    servo::embedder::scroll_by(id_, delta_x, delta_y);
}

void ServoNWeb::PutBackgroundColor(int color) {
    background_color_ = color;
}

std::shared_ptr<NWebPreference> ServoNWeb::GetPreference() {
    return GetServoPreference();
}

}  // namespace OHOS::NWeb
