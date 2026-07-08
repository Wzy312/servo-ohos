/*
 * Copyright (c) 2022 Huawei Device Co., Ltd.
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#ifndef NWEB_FULL_SCREEN_EXIT_HANDLER_H
#define NWEB_FULL_SCREEN_EXIT_HANDLER_H

#include <string>

#include "nweb_export.h"

namespace OHOS::NWeb {

class OHOS_NWEB_EXPORT NWebFullScreenExitHandler {
public:
    virtual ~NWebFullScreenExitHandler() = default;

    /**
     * @brief the handler to exit the full screen mode.
     */
    virtual void ExitFullScreen() = 0;
};

} // namespace OHOS::NWeb

#endif // NWEB_FULL_SCREEN_EXIT_HANDLER_H
