#ifndef SERVO_ARKWEB_PRELUDE_H
#define SERVO_ARKWEB_PRELUDE_H

// Force-included into every shim translation unit (see build.rs). Several vendored OHOS
// headers assume standard headers that happen to be transitively included in the OHOS build
// but not in isolation (e.g. nweb_drag_data.h uses UINT_MAX without including <climits>).
#include <climits>
#include <cstddef>
#include <cstdint>
#include <cstring>

#endif  // SERVO_ARKWEB_PRELUDE_H
