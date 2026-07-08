#ifndef SERVO_ARKWEB_SERVO_NWEB_H
#define SERVO_ARKWEB_SERVO_NWEB_H

#include <cstdint>
#include <memory>
#include <string>

#include "generated/servo_nweb_stub_base.h"
#include "servo_handler_proxy.h"

namespace OHOS::NWeb {

// One Servo-backed webview. Declared inside OHOS::NWeb so the overridden signatures match the
// vendored (unqualified) NWeb declarations verbatim. MVP methods forward to the cxx bridge
// keyed by the servo-side webview id; everything else falls through to the stub base.
class ServoNWeb : public ServoNWebStubBase {
public:
    ServoNWeb(uint32_t id, std::shared_ptr<servo::arkweb::NWebHandlerProxy> proxy);

    void Resize(uint32_t width, uint32_t height, bool isKeyboard) override;
    void OnPause() override;
    void OnContinue() override;
    void OnDestroy() override;
    void OnFocus(const FocusReason& focusReason) override;
    void OnBlur(const BlurReason& blurReason) override;
    void OnTouchPress(int32_t id, double x, double y, bool fromOverlay) override;
    void OnTouchRelease(int32_t id, double x, double y, bool fromOverlay) override;
    void OnTouchMove(int32_t id, double x, double y, bool fromOverlay) override;
    void OnTouchCancel() override;
    bool SendKeyEvent(int32_t keyCode, int32_t keyAction) override;
    int Load(const std::string& url) override;
    bool IsNavigatebackwardAllowed() override;
    bool IsNavigateForwardAllowed() override;
    bool CanNavigateBackOrForward(int numSteps) override;
    void NavigateBack() override;
    void NavigateForward() override;
    void Reload() override;
    int Zoom(float zoomFactor) override;
    void ExecuteJavaScript(const std::string& code) override;
    unsigned int GetWebId() override;
    void SetNWebHandler(std::shared_ptr<NWebHandler> handler) override;
    std::string GetUrl() override;
    void ScrollTo(float x, float y) override;
    void ScrollBy(float delta_x, float delta_y) override;
    void PutBackgroundColor(int color) override;
    std::shared_ptr<NWebPreference> GetPreference() override;

private:
    uint32_t id_;
    std::shared_ptr<servo::arkweb::NWebHandlerProxy> proxy_;
    int background_color_ = 0;
};

}  // namespace OHOS::NWeb

#endif  // SERVO_ARKWEB_SERVO_NWEB_H
