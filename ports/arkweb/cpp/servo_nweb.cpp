#include "servo_nweb.h"

#include <utility>

#include "arkweb/src/bridge.rs.h"
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

void ServoNWeb::OnTouchCancel() {
    servo::embedder::touch_event(id_, kTouchCancel, 0.0F, 0.0F, -1);
}

bool ServoNWeb::SendKeyEvent(int32_t keyCode, int32_t keyAction) {
    return servo::arkweb::send_key_event(id_, keyCode, keyAction);
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
