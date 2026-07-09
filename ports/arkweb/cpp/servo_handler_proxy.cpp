#include "servo_handler_proxy.h"

#include <utility>

#include "ohos_nweb/nweb_console_log.h"

namespace servo::arkweb {

namespace {
// Concrete NWebConsoleLog carrying a servo console message across to OnConsoleLog. Servo provides
// only the level and text; the line number and source id are unknown (0 / empty).
class ServoConsoleLog : public OHOS::NWeb::NWebConsoleLog {
public:
    ServoConsoleLog(std::string message, NWebConsoleLogLevel level)
        : message_(std::move(message)), level_(level) {}

    int LineNumer() override { return 0; }
    std::string Log() override { return message_; }
    NWebConsoleLogLevel LogLevel() override { return level_; }
    std::string SourceId() override { return {}; }

private:
    std::string message_;
    NWebConsoleLogLevel level_;
};
}  // namespace

void NWebHandlerProxy::set_handler(std::shared_ptr<OHOS::NWeb::NWebHandler> handler) {
    std::lock_guard<std::mutex> lock(mutex_);
    handler_ = std::move(handler);
}

std::shared_ptr<OHOS::NWeb::NWebHandler> NWebHandlerProxy::handler() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return handler_;
}

void NWebHandlerProxy::on_load_started(const std::string& url) const {
    if (auto h = handler()) {
        h->OnPageLoadBegin(url);
    }
}

void NWebHandlerProxy::on_load_finished(const std::string& url, std::int32_t http_status) const {
    if (auto h = handler()) {
        h->OnPageLoadEnd(static_cast<int>(http_status), url);
    }
}

void NWebHandlerProxy::on_load_error(std::int32_t code, const std::string& desc,
                                     const std::string& url) const {
    if (auto h = handler()) {
        h->OnPageLoadError(static_cast<int>(code), desc, url);
    }
}

// No direct NWebHandler equivalent yet; wired up in a later milestone.
void NWebHandlerProxy::on_url_changed(const std::string& /*url*/) const {}

void NWebHandlerProxy::on_title_changed(const std::string& title) const {
    if (auto h = handler()) {
        h->OnPageTitle(title);
    }
}

void NWebHandlerProxy::on_progress(std::int32_t progress) const {
    if (auto h = handler()) {
        h->OnLoadingProgress(static_cast<int>(progress));
    }
}

void NWebHandlerProxy::on_history_changed(bool /*can_back*/, bool /*can_fwd*/) const {}

void NWebHandlerProxy::on_console_message(std::int32_t level, const std::string& msg,
                                          std::int32_t /*line*/, const std::string& /*source*/) const {
    if (auto h = handler()) {
        auto log = std::make_shared<ServoConsoleLog>(
            msg, static_cast<OHOS::NWeb::NWebConsoleLog::NWebConsoleLogLevel>(level));
        h->OnConsoleLog(std::move(log));
    }
}

// Presentation is driven directly on the servo side (EGL swap); nothing to forward.
void NWebHandlerProxy::on_frame_ready() const {}

void NWebHandlerProxy::update_text_field_status(bool show_keyboard, bool attach_ime) const {
    if (auto h = handler()) {
        h->UpdateTextFieldStatus(show_keyboard, attach_ime);
    }
}

}  // namespace servo::arkweb
