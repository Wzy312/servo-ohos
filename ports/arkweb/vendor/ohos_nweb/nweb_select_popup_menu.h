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

#ifndef NWEB_SELECT_POPUP_MENU_H
#define NWEB_SELECT_POPUP_MENU_H

#include <memory>
#include <string>
#include <vector>

namespace OHOS::NWeb {

class NWebSelectMenuBound {
public:
    virtual ~NWebSelectMenuBound() = default;

    virtual int GetX() = 0;
    virtual int GetY() = 0;
    virtual int GetWidth() = 0;
    virtual int GetHeight() = 0;
};

enum SelectPopupMenuItemType {
    SP_OPTION,
    SP_CHECKABLE_OPTION,
    SP_GROUP,
    SP_SEPARATOR,
    SP_SUBMENU,
};

enum TextDirection {
    SP_UNKNOWN,
    SP_RTL,
    SP_LTR,
};

class NWebSelectPopupMenuItem {
public:
    virtual ~NWebSelectPopupMenuItem() = default;

    virtual SelectPopupMenuItemType GetType() = 0;

    virtual std::string GetLabel() = 0;

    virtual uint32_t GetAction() = 0;

    virtual std::string GetToolTip() = 0;

    virtual bool GetIsChecked() = 0;

    virtual bool GetIsEnabled() = 0;

    virtual TextDirection GetTextDirection() = 0;

    virtual bool GetHasTextDirectionOverride() = 0;
};

class NWebSelectPopupMenuParam {
public:
    virtual ~NWebSelectPopupMenuParam() = default;

    virtual std::vector<std::shared_ptr<NWebSelectPopupMenuItem>> GetMenuItems() = 0;

    virtual int GetItemHeight() = 0;

    virtual int GetSelectedItem() = 0;

    virtual double GetItemFontSize() = 0;

    virtual bool GetIsRightAligned() = 0;

    virtual std::shared_ptr<NWebSelectMenuBound> GetSelectMenuBound() = 0;

    virtual bool GetIsAllowMultipleSelection() = 0;
};

class NWebSelectPopupMenuCallback {
public:
    virtual ~NWebSelectPopupMenuCallback() = default;

    virtual void Continue(const std::vector<int32_t>& indices) = 0;

    virtual void Cancel() = 0;
};

} // namespace OHOS::NWeb

#endif