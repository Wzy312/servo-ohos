#include "servo_handler_proxy.h"

#include <utility>

namespace servo::arkweb {

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

// Forwarding requires constructing an NWebConsoleLog; deferred to the handler-breadth milestone.
void NWebHandlerProxy::on_console_message(std::int32_t /*level*/, const std::string& /*msg*/,
                                          std::int32_t /*line*/, const std::string& /*source*/) const {}

// Presentation is driven directly on the servo side (EGL swap); nothing to forward.
void NWebHandlerProxy::on_frame_ready() const {}

}  // namespace servo::arkweb
