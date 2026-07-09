#ifndef SERVO_ARKWEB_SERVO_HANDLER_PROXY_H
#define SERVO_ARKWEB_SERVO_HANDLER_PROXY_H

#include <cstdint>
#include <memory>
#include <mutex>
#include <string>

#include "ohos_nweb/nweb_handler.h"
#include "servo_client.h"

namespace servo::arkweb {

// Implements the embedder-generic WebViewClient sink by forwarding to the ACE-provided
// OHOS::NWeb::NWebHandler. The inner handler is swapped at runtime by NWeb::SetNWebHandler.
class NWebHandlerProxy : public servo::embedder::WebViewClient {
public:
    void set_handler(std::shared_ptr<OHOS::NWeb::NWebHandler> handler);

    void on_load_started(const std::string& url) const override;
    void on_load_finished(const std::string& url, std::int32_t http_status) const override;
    void on_load_error(std::int32_t code, const std::string& desc,
                       const std::string& url) const override;
    void on_url_changed(const std::string& url) const override;
    void on_title_changed(const std::string& title) const override;
    void on_progress(std::int32_t progress) const override;
    void on_history_changed(bool can_back, bool can_fwd) const override;
    void on_console_message(std::int32_t level, const std::string& msg, std::int32_t line,
                            const std::string& source) const override;
    void on_frame_ready() const override;
    void update_text_field_status(bool show_keyboard, bool attach_ime) const override;
    bool show_js_dialog(std::uint64_t dialog_id, std::int32_t kind, const std::string& message,
                        const std::string& default_value) const override;
    bool show_select_popup(std::uint64_t select_id, const std::string& labels, std::int32_t selected,
                           bool multiple, std::int32_t x, std::int32_t y, std::int32_t width,
                           std::int32_t height) const override;

private:
    std::shared_ptr<OHOS::NWeb::NWebHandler> handler() const;

    mutable std::mutex mutex_;
    std::shared_ptr<OHOS::NWeb::NWebHandler> handler_;
};

}  // namespace servo::arkweb

#endif  // SERVO_ARKWEB_SERVO_HANDLER_PROXY_H
