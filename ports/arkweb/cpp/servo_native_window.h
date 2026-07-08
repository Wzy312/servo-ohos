#pragma once

#include <cstddef>
#include <cstdint>

namespace servo::arkweb {

// Set the buffer geometry of an OHNativeWindow (passed as its address). surfman creates the EGL
// window surface with `CreatePlatformWindowSurface` and trusts the native window's existing buffer
// geometry; ACE's producer surface has no geometry set, so this must be called before the surface
// is created (and on resize) or WebRender renders into degenerate buffers and reports OutOfMemory.
void set_native_window_buffer_geometry(std::size_t window, std::uint32_t width, std::uint32_t height);

}  // namespace servo::arkweb
