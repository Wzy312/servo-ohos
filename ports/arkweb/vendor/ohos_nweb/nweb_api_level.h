/*
 * Copyright (c) 2024 Huawei Device Co., Ltd.
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

#ifndef NWEB_API_LEVEL_H_
#define NWEB_API_LEVEL_H_

/*
 * API level consists of API version and ArkWeb monthly version. The last three digits
 * are ArkWeb monthly version. If the ArkWeb minor version is less than three digits, it
 * is padded with 0. For example, if the API version is 12 and the ArkWeb monthly version
 * is 1, then API level is 12001.
 */
#define ARKWEB_CORE_API_LEVEL 13001

#endif  // NWEB_API_LEVEL_H_

