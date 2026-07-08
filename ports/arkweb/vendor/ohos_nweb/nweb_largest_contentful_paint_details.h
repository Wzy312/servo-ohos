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

#ifndef NWEB_LARGEST_CONTENTFUL_PAINT_DETAILS_H
#define NWEB_LARGEST_CONTENTFUL_PAINT_DETAILS_H

#include <math.h>
#include <string>

#include "nweb_export.h"

namespace OHOS::NWeb {

class OHOS_NWEB_EXPORT NWebLargestContentfulPaintDetails {
public:
    NWebLargestContentfulPaintDetails() = default;

    virtual ~NWebLargestContentfulPaintDetails() = default;

    /**
     * @brief Get start time of navigation.
     *
     * @retval Start time of navigation.
     */
    virtual int64_t GetNavigationStartTime() = 0;

    /**
     * @brief Get paint time of largest image.
     *
     * @retval Paint time of largest image..
     */
    virtual int64_t GetLargestImagePaintTime() = 0;

    /**
     * @brief Get paint time of largest text.
     *
     * @retval Paint time of largest text.
     */
    virtual int64_t GetLargestTextPaintTime() = 0;

    /**
     * @brief Get load start time of largest image.
     *
     * @retval Load start time of largest image.
     */
    virtual int64_t GetLargestImageLoadStartTime() = 0;

    /**
     * @brief Get load end time of largest image.
     *
     * @retval Load end time of largest image.
     */
    virtual int64_t GetLargestImageLoadEndTime() = 0;

    /**
     * @brief Get bits per pixel of image.
     *
     * @retval Bits per pixel of image.
     */
    virtual double_t GetImageBPP() = 0;
};

} // namespace OHOS::NWeb

#endif // NWEB_LARGEST_CONTENTFUL_PAINT_DETAILS_H