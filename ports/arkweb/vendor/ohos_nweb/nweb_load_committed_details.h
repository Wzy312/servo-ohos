/*
 * Copyright (c) 2023 Huawei Device Co., Ltd.
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

#ifndef NWEB_LOAD_COMMITTED_DETAILS_H
#define NWEB_LOAD_COMMITTED_DETAILS_H

#include <map>
#include <string>

#include "nweb_export.h"

namespace OHOS::NWeb {

class OHOS_NWEB_EXPORT NWebLoadCommittedDetails {
public:
    enum NavigationType {
        NAVIGATION_TYPE_UNKNOWN = 0,
        NAVIGATION_TYPE_MAIN_FRAME_NEW_ENTRY = 1,
        NAVIGATION_TYPE_MAIN_FRAME_EXISTING_PAGE = 2,
        NAVIGATION_TYPE_NEW_SUBFRAME = 4,
        NAVIGATION_TYPE_AUTO_SUBFRAME = 5,
    };

    NWebLoadCommittedDetails() = default;

    virtual ~NWebLoadCommittedDetails() = default;

    /**
     * @brief Check whether the request is for getting the main frame.
     *
     * @retval Is main frame.
     */
    virtual bool IsMainFrame() = 0;

    /**
     * @brief Check whether document and other documents have the same
     * properties.
     *
     * @retval Is the same document.
     */
    virtual bool IsSameDocument() = 0;

    /**
     * @brief Check whether the entry is replaced.
     *
     * @retval The entry is replaced.
     */
    virtual bool DidReplaceEntry() = 0;

    /**
     * @brief Get the value of the navigation type.
     *
     * @retval The value of the navigation type.
     */
    virtual NavigationType GetNavigationType() = 0;

    /**
     * @brief Gets the url of the current navigation.
     *
     * @retval The url of the current navigation.
     */
    virtual std::string GetURL() = 0;
};

} // namespace OHOS::NWeb

#endif // NWEB_LOAD_COMMITTED_DETAILS_H
