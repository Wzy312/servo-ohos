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

#ifndef NWEB_DATE_TIME_CHOOSER_H
#define NWEB_DATE_TIME_CHOOSER_H

#include <memory>
#include <string>
#include <vector>

namespace OHOS::NWeb {

struct DateTime {
    int32_t year = 0;
    int32_t month = 0;
    int32_t day = 0;
    int32_t hour = 0;
    int32_t minute = 0;
    int32_t second = 0;
};

class NWebDateTimeSuggestion {
public:
    virtual ~NWebDateTimeSuggestion() = default;

    virtual DateTime GetValue() = 0;

    virtual std::string GetLabel() = 0;

    virtual std::string GetLocalizedValue() = 0;
};

enum DateTimeChooserType { DTC_DATE, DTC_DATETIME, DTC_DATETIME_LOCAL, DTC_TIME, DTC_MONTH, DTC_WEEK, DTC_UNKNOWN };

class NWebDateTimeChooser {
public:
    virtual ~NWebDateTimeChooser() = default;

    virtual DateTimeChooserType GetType() = 0;

    virtual double GetStep() = 0;

    virtual DateTime GetMinimum() = 0;

    virtual DateTime GetMaximum() = 0;

    virtual DateTime GetDialogValue() = 0;

    virtual bool GetHasSelected() = 0;

    virtual size_t GetSuggestionIndex() = 0;
};

class NWebDateTimeChooserCallback {
public:
    virtual ~NWebDateTimeChooserCallback() = default;

    virtual void Continue(bool success, const DateTime& value) = 0;
};

} // namespace OHOS::NWeb

#endif