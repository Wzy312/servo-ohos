/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::time::{Duration, SystemTime};

use headers::{Expires, HeaderMapExt, LastModified};
use http::header::CACHE_CONTROL;
use http::{HeaderMap, StatusCode, header};
use net_traits::request::{CacheMode, Request};
use net_traits::response::Response;

/// Holds the freshness and storability decisions for one response so the rest
/// of the cache can keep its RFC 9111 logic in one place.
#[derive(Clone, Debug)]
pub(crate) struct HttpCacheSemantics {
    freshness_lifetime: Duration,
    has_expires: bool,
    has_last_modified: bool,
    has_etag: bool,
    has_max_age: bool,
    has_no_cache: bool,
    has_no_store: bool,
    has_public: bool,
    has_pragma_no_cache: bool,
}

impl HttpCacheSemantics {
    pub(crate) fn new(response: &Response) -> Self {
        let actual_response = response.actual_response();
        let headers = &actual_response.headers;
        let has_expires = headers.contains_key(header::EXPIRES);
        let has_last_modified = headers.contains_key(header::LAST_MODIFIED);
        let has_etag = headers.contains_key(header::ETAG);
        let has_pragma_no_cache = headers
            .typed_get::<headers::Pragma>()
            .is_some_and(|pragma| pragma.is_no_cache());
        let mut has_max_age = false;
        let mut has_no_cache = false;
        let mut has_no_store = false;
        let mut has_public = false;
        let age = response_age(headers);
        let mut explicit_freshness = Duration::ZERO;
        let mut heuristic_lifetime = Duration::ZERO;

        for value in headers.get_all(CACHE_CONTROL) {
            let Ok(value) = value.to_str() else {
                continue;
            };

            for directive in value.split(',') {
                let directive = directive.trim();
                if directive.eq_ignore_ascii_case("no-cache") {
                    has_no_cache = true;
                }

                if directive.eq_ignore_ascii_case("no-store") {
                    has_no_store = true;
                    continue;
                }

                if directive.eq_ignore_ascii_case("public") {
                    has_public = true;
                }

                if let Some((name, _argument)) = directive.split_once('=') {
                    if name.trim().eq_ignore_ascii_case("max-age") ||
                        name.trim().eq_ignore_ascii_case("s-maxage")
                    {
                        has_max_age = true;
                    }
                }
            }
        }

        let has_explicit_freshness = has_expires || has_max_age;

        if let Some(seconds) = max_age_or_s_maxage(headers) {
            explicit_freshness = seconds.saturating_sub(age);
        } else if headers.contains_key(header::EXPIRES) {
            explicit_freshness = expires_freshness(headers).saturating_sub(age);
        }

        // Statuses that are cacheable by default may take heuristic freshness.
        // Any other status needs the public cache directive to take it.
        if has_last_modified &&
            (is_cacheable_by_default(actual_response.status.code()) || has_public)
        {
            heuristic_lifetime = heuristic_response_freshness(headers);
        }

        Self {
            freshness_lifetime: if has_no_cache {
                Duration::ZERO
            } else if has_explicit_freshness {
                explicit_freshness
            } else {
                heuristic_lifetime
            },
            has_expires,
            has_last_modified,
            has_etag,
            has_max_age,
            has_no_cache,
            has_no_store,
            has_public,
            has_pragma_no_cache,
        }
    }

    /// Whether the response may be stored, following the same order of
    /// decisions the cache used before the store was made pluggable.
    /// <https://www.rfc-editor.org/rfc/rfc9111#section-3>
    pub(crate) fn is_cacheable(&self) -> bool {
        if self.has_no_store {
            return false;
        }

        // A Cache-Control header carrying one of these settles the question on
        // its own, and pragma is then ignored.
        if self.has_public || self.has_max_age || self.has_no_cache {
            return true;
        }

        if self.has_pragma_no_cache {
            return false;
        }

        self.has_expires || self.has_last_modified || self.has_etag
    }

    pub(crate) fn freshness_lifetime(&self) -> Duration {
        self.freshness_lifetime
    }
}

/// Determine whether the request itself demands revalidation.
pub(crate) fn request_demands_revalidation(request: &Request) -> bool {
    if matches!(
        request.cache_mode,
        CacheMode::NoCache | CacheMode::Reload | CacheMode::NoStore
    ) {
        return true;
    }

    for value in request.headers.get_all(CACHE_CONTROL) {
        let Ok(value) = value.to_str() else {
            continue;
        };

        for directive in value.split(',') {
            let directive = directive.trim();
            if directive.eq_ignore_ascii_case("no-cache") {
                return true;
            }

            if let Some((name, argument)) = directive.split_once('=') {
                if name.trim().eq_ignore_ascii_case("max-age") &&
                    argument
                        .trim()
                        .trim_matches('"')
                        .parse::<u64>()
                        .is_ok_and(|seconds| seconds == 0)
                {
                    return true;
                }
            }
        }
    }

    false
}

/// Status codes a cache may store without explicit freshness information.
/// <https://www.rfc-editor.org/rfc/rfc9110#section-15.1>
fn is_cacheable_by_default(status_code: StatusCode) -> bool {
    matches!(
        status_code.as_u16(),
        200 | 203 | 204 | 206 | 300 | 301 | 404 | 405 | 410 | 414 | 501
    )
}

fn response_age(headers: &HeaderMap) -> Duration {
    headers
        .get(header::AGE)
        .and_then(|age_header| age_header.to_str().ok())
        .and_then(|age_string| age_string.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_default()
}

fn max_age_or_s_maxage(headers: &HeaderMap) -> Option<Duration> {
    for value in headers.get_all(CACHE_CONTROL) {
        let Ok(value) = value.to_str() else {
            continue;
        };

        for directive in value.split(',') {
            let directive = directive.trim();
            if let Some((name, argument)) = directive.split_once('=') {
                if name.trim().eq_ignore_ascii_case("s-maxage") ||
                    name.trim().eq_ignore_ascii_case("max-age")
                {
                    if let Ok(seconds) = argument.trim().trim_matches('"').parse::<u64>() {
                        return Some(Duration::from_secs(seconds));
                    }
                }
            }
        }
    }

    None
}

fn expires_freshness(headers: &HeaderMap) -> Duration {
    headers
        .typed_get::<Expires>()
        .map(|expiry| {
            let expiry_time: SystemTime = expiry.into();
            expiry_time
                .duration_since(SystemTime::now())
                .unwrap_or(Duration::ZERO)
        })
        .unwrap_or_default()
}

fn heuristic_response_freshness(headers: &HeaderMap) -> Duration {
    let Some(last_modified) = headers.typed_get::<LastModified>() else {
        // Compatible with other browsers.
        return Duration::ZERO;
    };

    let last_modified: SystemTime = last_modified.into();
    let time_since_last_modified = SystemTime::now()
        .duration_since(last_modified)
        .unwrap_or_default();
    let heuristic_freshness = time_since_last_modified / 10;
    let max_heuristic = Duration::from_secs(24 * 60 * 60);
    heuristic_freshness.min(max_heuristic)
}
