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
    // Asks ACE to show a <select> dropdown (labels joined by '\n'; selected = current index or -1;
    // x/y/width/height = the select's on-screen rect). ACE renders the menu itself and reports the
    // choice via servo::arkweb::select_popup_continue / select_popup_cancel.
    virtual bool show_select_popup(std::uint64_t select_id, const std::string& labels,
                                   std::int32_t selected, bool multiple, std::int32_t x,
                                   std::int32_t y, std::int32_t width, std::int32_t height) const = 0;
    // Asks ACE to show an <input type=file> picker (accept filters joined by '\n', empty for none;
    // multiple = allow several files). Returns whether a handler took it; the chosen paths arrive
    // via servo::arkweb::file_picker_continue / file_picker_cancel.
    virtual bool show_file_picker(std::uint64_t picker_id, const std::string& accept_types,
                                  bool multiple) const = 0;
    // Asks ACE to show a geolocation permission prompt for `origin` (ArkTS onGeolocationShow).
    // Returns whether a handler took it; the decision arrives via servo::arkweb::resolve_permission.
    virtual bool show_geolocation_permission(std::uint64_t request_id,
                                             const std::string& origin) const = 0;
    // Asks ACE to show a permission prompt (ArkTS onPermissionRequest). `resources` is the
    // NWebAccessRequest resource bitmask (1<<1 video capture, 1<<2 audio capture). Returns whether
    // a handler took it; the decision arrives via servo::arkweb::resolve_permission.
    virtual bool show_permission_request(std::uint64_t request_id, const std::string& origin,
                                         std::int32_t resources) const = 0;
    // Asks ACE to show an HTTP-auth login prompt (ArkTS onHttpAuthRequest). Returns whether a
    // handler took it; credentials arrive via servo::arkweb::resolve_http_auth.
    virtual bool show_http_auth_request(std::uint64_t request_id, const std::string& host,
                                        const std::string& realm) const = 0;
};

}  // namespace servo::embedder

#endif  // SERVO_ARKWEB_SERVO_CLIENT_H
