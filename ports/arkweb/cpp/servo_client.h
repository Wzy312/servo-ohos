#ifndef SERVO_ARKWEB_SERVO_CLIENT_H
#define SERVO_ARKWEB_SERVO_CLIENT_H

#include <cstdint>
#include <string>

namespace servo::embedder {

// Abstract engine -> embedder callback sink, invoked from the servo side through the cxx
// bridge (declared there as an opaque `WebViewClient`). `NWebHandlerProxy` implements it by
// forwarding to the ACE-provided OHOS::NWeb::NWebHandler.
//
// Methods are const: invoking a callback does not mutate the sink itself (the mutable inner
// handler pointer is guarded separately). This matches the cxx `self: &WebViewClient` binding.
class WebViewClient {
public:
    virtual ~WebViewClient() = default;

    virtual void on_load_started(const std::string& url) const = 0;
    virtual void on_load_finished(const std::string& url, std::int32_t http_status) const = 0;
    virtual void on_load_error(std::int32_t code, const std::string& desc,
                               const std::string& url) const = 0;
    virtual void on_url_changed(const std::string& url) const = 0;
    virtual void on_title_changed(const std::string& title) const = 0;
    virtual void on_progress(std::int32_t progress) const = 0;
    virtual void on_history_changed(bool can_back, bool can_fwd) const = 0;
    virtual void on_console_message(std::int32_t level, const std::string& msg, std::int32_t line,
                                    const std::string& source) const = 0;
    virtual void on_frame_ready() const = 0;
    // Notifies ACE that an editable is focused / blurred so it tracks the focus text field and its
    // back button closes the soft keyboard (the keyboard itself is driven by the engine's own IME).
    virtual void update_text_field_status(bool show_keyboard, bool attach_ime) const = 0;
    // Asks ACE to show a JS dialog (kind: 0=alert, 1=confirm, 2=prompt). Returns whether a handler
    // took it; the user's response is delivered later via servo::arkweb::resolve_js_dialog.
    virtual bool show_js_dialog(std::uint64_t dialog_id, std::int32_t kind,
                                const std::string& message,
                                const std::string& default_value) const = 0;
};

}  // namespace servo::embedder

#endif  // SERVO_ARKWEB_SERVO_CLIENT_H
