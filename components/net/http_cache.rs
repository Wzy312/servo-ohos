/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![deny(missing_docs)]

//! A memory cache implementing the logic specified in <http://tools.ietf.org/html/rfc7234>
//! and <http://tools.ietf.org/html/rfc7232>.

use std::collections::HashMap;
use std::ops::Bound;
use std::sync::Arc as StdArc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use bytes::Bytes;
use futures_util::StreamExt;
use headers::{
    CacheControl, ContentRange, HeaderMapExt, IfModifiedSince, LastModified, Range, Vary,
};
use http::{HeaderMap, Method, StatusCode, header};
use log::debug;
use malloc_size_of::{MallocSizeOf, MallocSizeOfOps};
use malloc_size_of_derive::MallocSizeOf;
use net_traits::http_status::HttpStatus;
use net_traits::request::{CacheMode, Request, RequestMode};
use net_traits::response::{CacheState, Response, ResponseBody};
use net_traits::{CacheEntryDescriptor, FetchMetadata, ResourceFetchTiming};
use parking_lot::{Mutex as ParkingLotMutex, RwLock as ParkingLotRwLock};
use servo_arc::Arc;
use servo_config::pref;
use servo_url::ServoUrl;
use tokio::sync::mpsc::{UnboundedSender as TokioSender, unbounded_channel as unbounded};
use tokio::sync::watch;

use crate::async_runtime::spawn_blocking_task;
use crate::fetch::methods::{Data, DoneChannel, FetchContext};
use crate::http_cache_semantics::{HttpCacheSemantics, request_demands_revalidation};
use crate::http_cache_store::{
    BodyHandle, BodyWriter, HttpCacheStore, MemoryStore, StoreError, StoredCachePolicy,
    StoredVariant, StoredVariantMeta, sanitized_request_headers,
};
use crate::http_loader::spawn_stale_while_revalidate;

/// The key used to differentiate requests in the cache.
#[derive(Clone, Eq, Hash, MallocSizeOf, PartialEq)]
pub struct CacheKey {
    url: ServoUrl,
}

impl CacheKey {
    /// Create a cache-key from a request.
    pub fn new(request: &Request) -> CacheKey {
        CacheKey {
            url: request.current_url(),
        }
    }

    /// Create a cache-key from a resolved URL.
    pub fn from_url(url: ServoUrl) -> CacheKey {
        CacheKey { url }
    }

    /// Return the resolved URL used by this cache key.
    pub fn url(&self) -> &ServoUrl {
        &self.url
    }
}

fn normalize_cached_response_headers(headers: &mut HeaderMap) {
    headers.remove(header::CONTENT_ENCODING);
    headers.remove(header::CONTENT_LENGTH);
}

/// A complete cached resource.
#[derive(Clone, MallocSizeOf)]
pub struct CachedResource {
    #[conditional_malloc_size_of]
    /// Request headers used for Vary matching.
    pub(crate) request_headers: Arc<ParkingLotMutex<HeaderMap>>,
    #[conditional_malloc_size_of]
    /// The cached response body.
    pub(crate) body: Arc<ParkingLotMutex<ResponseBody>>,
    #[conditional_malloc_size_of]
    /// Handle for updating the persisted entry metadata.
    pub(crate) body_handle: BodyHandle,
    #[conditional_malloc_size_of]
    /// Whether the entry was aborted while being fetched.
    pub(crate) aborted: Arc<AtomicBool>,
    #[conditional_malloc_size_of]
    /// Consumers waiting for the body to finish.
    pub(crate) awaiting_body: Arc<ParkingLotMutex<Vec<TokioSender<Data>>>>,
    /// Response metadata needed to reconstruct a hit.
    pub(crate) metadata: CachedMetadata,
    #[ignore_malloc_size_of = "HttpCacheSemantics"]
    /// Cache policy snapshot for freshness and revalidation.
    pub(crate) cache_semantics: HttpCacheSemantics,
    /// Final URL associated with the cached response.
    pub(crate) location_url: Option<Result<ServoUrl, String>>,
    /// Cached HTTP status.
    pub(crate) status: HttpStatus,
    /// Stable body length used for cache weighting.
    pub(crate) body_len: usize,
    /// URL chain for the cached response.
    pub(crate) url_list: Vec<ServoUrl>,
    /// Freshness lifetime.
    pub(crate) expires: Duration,
    /// Stale-while-revalidate window.
    pub(crate) stale_while_revalidate: Duration,
    #[conditional_malloc_size_of]
    /// Revalidation state shared across consumers.
    pub(crate) revalidating: StdArc<AtomicBool>,
    /// Last validation timestamp.
    pub(crate) last_validated: Instant,
}

/// Metadata about a loaded resource, such as is obtained from HTTP headers.
#[derive(Clone, MallocSizeOf)]
pub(crate) struct CachedMetadata {
    /// Headers
    #[conditional_malloc_size_of]
    /// Response headers.
    pub(crate) headers: Arc<ParkingLotMutex<HeaderMap>>,
    /// Final URL after redirects.
    ///
    /// This is the URL exposed to consumers.
    pub(crate) final_url: ServoUrl,
    /// MIME type / subtype.
    ///
    /// Stored as a string because it comes from the metadata layer.
    pub(crate) content_type: Option<String>,
    /// Character set.
    ///
    /// Stored as an owned string for later reconstruction.
    pub(crate) charset: Option<String>,
    /// HTTP Status
    ///
    /// This matches the cached response status.
    pub(crate) status: HttpStatus,
}

impl CachedResource {
    /// Return the cached body length.
    pub fn body_len(&self) -> usize {
        self.body_len
    }

    /// Return the cached body handle.
    pub fn body(&self) -> Arc<ParkingLotMutex<ResponseBody>> {
        self.body.clone()
    }

    /// Return the response final URL.
    pub fn final_url(&self) -> ServoUrl {
        self.metadata.final_url.clone()
    }

    /// Return the cached HTTP status.
    pub fn status(&self) -> HttpStatus {
        self.status.clone()
    }

    /// Update the final URL.
    pub fn set_final_url(&mut self, final_url: ServoUrl) {
        self.metadata.final_url = final_url;
    }

    /// Update the cached HTTP status.
    pub fn set_status(&mut self, status: HttpStatus) {
        self.metadata.status = status.clone();
        self.status = status;
    }
}

impl StoredVariantMeta {
    pub(crate) fn into_cached_resource(self, body_handle: BodyHandle) -> CachedResource {
        let timing =
            net_traits::ResourceFetchTiming::new(net_traits::ResourceTimingType::Navigation);
        let mut response = Response::new(self.final_url.clone(), timing);
        response.headers = self.response_headers.clone();
        response.status = self.status.clone();
        response.location_url = self.location_url.clone();

        let cache_semantics = HttpCacheSemantics::new(&response);
        CachedResource {
            request_headers: Arc::new(ParkingLotMutex::new(self.request_headers)),
            body: Arc::new(ParkingLotMutex::new(ResponseBody::Empty)),
            body_handle,
            aborted: Arc::new(AtomicBool::new(false)),
            awaiting_body: Arc::new(ParkingLotMutex::new(vec![])),
            metadata: CachedMetadata {
                headers: Arc::new(ParkingLotMutex::new(response.headers.clone())),
                final_url: self.final_url.clone(),
                content_type: self.content_type,
                charset: self.charset,
                status: self.status.clone(),
            },
            cache_semantics,
            location_url: self.location_url,
            status: self.status,
            body_len: self.body_len,
            url_list: self.url_list,
            expires: self.expires,
            stale_while_revalidate: self.stale_while_revalidate,
            revalidating: StdArc::new(AtomicBool::new(false)),
            last_validated: Instant::now(),
        }
    }
}

/// Whether a cached response is fresh or requires validation before or after use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationStatus {
    /// The response is fresh and can be used without any revalidation.
    Valid,
    /// The response is stale.
    Stale {
        /// Whether the stale response can be served immediately, leaving the
        /// caller responsible for revalidating it in the background.
        revalidate_in_background: bool,
    },
}

/// Wrapper around a cached response, including information on re-validation needs
pub(crate) struct CachedResponse {
    /// The response constructed from the cached resource
    pub response: Response,
    /// Whether the stored response is fresh or stale
    pub validation_status: ValidationStatus,
    /// Single-flight guard for the background revalidation.
    pub revalidation_guard: StdArc<AtomicBool>,
}

struct InFlightEntry {
    state: watch::Sender<bool>,
}

type LiveEntry = StdArc<ParkingLotRwLock<Vec<CachedResource>>>;

impl InFlightEntry {
    fn new() -> Self {
        let (state, _) = watch::channel(false);
        Self { state }
    }
}

/// The result of requesting single-flight coordination for a cache key.
pub enum InFlightReservation<'a> {
    /// This request owns the in-flight fetch.
    Producer(InFlightLease<'a>),
    /// This request waits for the current producer to finish.
    Waiter(watch::Receiver<bool>),
}

impl<'a> InFlightReservation<'a> {
    /// Wait until the current producer finishes.
    pub async fn wait(self) {
        if let Self::Waiter(mut state) = self {
            if !*state.borrow() {
                let _ = state.changed().await;
            }
        }
    }
}

/// A producer lease for an in-flight cache fetch.
pub struct InFlightLease<'a> {
    cache: &'a HttpCache,
    key: CacheKey,
}

impl<'a> InFlightLease<'a> {
    fn new(cache: &'a HttpCache, key: CacheKey) -> Self {
        Self { cache, key }
    }
}

impl Drop for InFlightLease<'_> {
    fn drop(&mut self) {
        self.cache.finish_inflight(&self.key);
    }
}

/// HTTP cache orchestration over the configured storage backend.
pub struct HttpCache {
    /// Cached responses.
    store: Box<dyn HttpCacheStore>,
    /// Live cached responses owned by the cache orchestration layer.
    live_entries: StdArc<ParkingLotMutex<HashMap<CacheKey, LiveEntry>>>,
    /// Active fetches keyed by cache key.
    in_flight: ParkingLotMutex<HashMap<CacheKey, InFlightEntry>>,
}

impl MallocSizeOf for HttpCache {
    fn size_of(&self, ops: &mut MallocSizeOfOps) -> usize {
        let in_flight_size: usize = {
            let in_flight = self.in_flight.lock();
            in_flight
                .iter()
                .map(|(key, _entry)| key.size_of(ops) + std::mem::size_of::<InFlightEntry>())
                .sum()
        };
        let live_entries_size: usize = {
            let live_entries = self.live_entries.lock();
            live_entries
                .iter()
                .map(|(key, entry)| {
                    key.size_of(ops) + entry.try_read().map(|lock| lock.size_of(ops)).unwrap_or(0)
                })
                .sum()
        };
        self.store.size_of(ops) + in_flight_size + live_entries_size
    }
}

impl Default for HttpCache {
    fn default() -> Self {
        Self {
            store: Box::new(MemoryStore::default()),
            live_entries: StdArc::new(ParkingLotMutex::new(HashMap::new())),
            in_flight: ParkingLotMutex::new(HashMap::new()),
        }
    }
}

/// The `headers` crate's `CacheControl` does not understand `stale-while-revalidate` directive,
/// so we need to parse the raw `Cache-Control` header values.
/// <https://datatracker.ietf.org/doc/html/rfc5861#section-3>
fn get_stale_while_revalidate(headers: &HeaderMap) -> Duration {
    for value in headers.get_all(header::CACHE_CONTROL) {
        let Ok(value) = value.to_str() else {
            continue;
        };
        for directive in value.split(',') {
            let directive = directive.trim();
            let Some((name, argument)) = directive.split_once('=') else {
                continue;
            };
            if !name.trim().eq_ignore_ascii_case("stale-while-revalidate") {
                continue;
            }
            // The argument is a number of seconds, optionally quoted.
            let argument = argument.trim().trim_matches('"');
            if let Ok(seconds) = argument.parse::<u64>() {
                return Duration::from_secs(seconds);
            }
        }
    }
    Duration::ZERO
}

/// Request Cache-Control Directives
/// <https://tools.ietf.org/html/rfc7234#section-5.2.1>
fn get_expiry_adjustment_from_request_headers(request: &Request, expires: Duration) -> Duration {
    let Some(directive) = request.headers.typed_get::<CacheControl>() else {
        return expires;
    };

    if let Some(max_age) = directive.max_stale() {
        return expires + max_age;
    }

    match directive.max_age() {
        Some(max_age) if expires > max_age => return Duration::ZERO,
        Some(max_age) => return expires - max_age,
        None => {},
    };

    if let Some(min_fresh) = directive.min_fresh() {
        if expires < min_fresh {
            return Duration::ZERO;
        }
        return expires - min_fresh;
    }

    if directive.no_cache() || directive.no_store() {
        return Duration::ZERO;
    }

    expires
}

/// Create a CachedResponse from a request and a CachedResource.
fn create_cached_response(
    request: &Request,
    cached_resource: &CachedResource,
    cached_headers: &HeaderMap,
    _done_chan: &mut DoneChannel,
) -> Option<CachedResponse> {
    debug!("creating a cached response for {:?}", request.url());
    if cached_resource.aborted.load(Ordering::Acquire) {
        return None;
    }
    // A variant whose body is still being written is not selectable. Late
    // consumers wait on the in-flight reservation for the producing fetch and
    // read the entry once it is complete, rather than attaching to the growing
    // buffer. That is the behaviour the task document specifies for a disk
    // backend, applied to the memory backend as well: sharing a half-written
    // buffer across the tee boundary is what produced the document-context
    // stalls this phase had to fix, and the reservation gives the same
    // dogpile protection without reintroducing that race.
    if matches!(*cached_resource.body.lock(), ResponseBody::Receiving(_)) {
        return None;
    }
    let resource_timing = ResourceFetchTiming::new(request.timing_type());
    let mut response = Response::new(cached_resource.metadata.final_url.clone(), resource_timing);
    response.headers = cached_headers.clone();
    response.body = cached_resource.body.clone();
    response
        .location_url
        .clone_from(&cached_resource.location_url);
    response.status.clone_from(&cached_resource.status);
    response.url_list.clone_from(&cached_resource.url_list);
    response.referrer = request.referrer.to_url().cloned();
    response.referrer_policy = request.referrer_policy;
    response.aborted = cached_resource.aborted.clone();

    let expires = cached_resource.expires;
    let adjusted_expires = get_expiry_adjustment_from_request_headers(request, expires);
    let time_since_validated = Instant::now() - cached_resource.last_validated;

    // TODO: take must-revalidate into account <https://tools.ietf.org/html/rfc7234#section-5.2.2.1>
    // TODO: if this cache is to be considered shared, take proxy-revalidate into account
    // <https://tools.ietf.org/html/rfc7234#section-5.2.2.7>
    let has_expired = adjusted_expires <= time_since_validated;

    // - fresh: return immediately, no validation.
    // - stale:
    //    - within the stale-while-revalidate window: return immediately + revalidate in the background
    //    - beyond the stale-while-revalidate window: synchronous validation is required.
    let stale_for = time_since_validated.saturating_sub(adjusted_expires);
    let within_stale_while_revalidate_window = stale_for <= cached_resource.stale_while_revalidate;
    let validation_status = if !has_expired {
        ValidationStatus::Valid
    } else {
        ValidationStatus::Stale {
            revalidate_in_background: within_stale_while_revalidate_window &&
                !cached_resource.stale_while_revalidate.is_zero() &&
                !request_demands_revalidation(request),
        }
    };

    let cached_response = CachedResponse {
        response,
        validation_status,
        revalidation_guard: cached_resource.revalidating.clone(),
    };
    Some(cached_response)
}

/// Create a new resource, based on the bytes requested, and an existing resource,
/// with a status-code of 206.
fn create_resource_with_bytes_from_resource(
    bytes: &[u8],
    resource: &CachedResource,
) -> CachedResource {
    CachedResource {
        request_headers: resource.request_headers.clone(),
        body: Arc::new(ParkingLotMutex::new(ResponseBody::Done(bytes.to_owned()))),
        body_handle: resource.body_handle.clone(),
        aborted: Arc::new(AtomicBool::new(false)),
        awaiting_body: Arc::new(ParkingLotMutex::new(vec![])),
        metadata: resource.metadata.clone(),
        cache_semantics: resource.cache_semantics.clone(),
        location_url: resource.location_url.clone(),
        status: StatusCode::PARTIAL_CONTENT.into(),
        body_len: bytes.len(),
        url_list: resource.url_list.clone(),
        expires: resource.expires,
        stale_while_revalidate: resource.stale_while_revalidate,
        revalidating: resource.revalidating.clone(),
        last_validated: resource.last_validated,
    }
}

/// Support for range requests <https://tools.ietf.org/html/rfc7233>.
///
/// Range handling sits in front of the policy layer: `construct_response`
/// dispatches to this function before consulting [`HttpCacheSemantics`], so a
/// range request is satisfied from the stored bytes. Partial (206) variants are
/// stored and selected by this function alone.
fn handle_range_request(
    request: &Request,
    candidates: &[&CachedResource],
    range_spec: &Range,
    done_chan: &mut DoneChannel,
) -> Option<CachedResponse> {
    let mut complete_cached_resources = candidates
        .iter()
        .filter(|resource| resource.status == StatusCode::OK);
    let partial_cached_resources = candidates
        .iter()
        .filter(|resource| resource.status == StatusCode::PARTIAL_CONTENT);
    if let Some(complete_resource) = complete_cached_resources.next() {
        // TODO: take the full range spec into account.
        // If we have a complete resource, take the request range from the body.
        // When there isn't a complete resource available, we loop over cached partials,
        // and see if any individual partial response can fulfill the current request for a bytes range.
        // TODO: combine partials that in combination could satisfy the requested range?
        // see <https://tools.ietf.org/html/rfc7233#section-4.3>.
        // TODO: add support for complete and partial resources,
        // whose body is in the ResponseBody::Receiving state.
        let body_len = match *complete_resource.body.lock() {
            ResponseBody::Done(ref body) => body.len(),
            _ => 0,
        };
        let bound = range_spec
            .satisfiable_ranges(body_len.try_into().unwrap())
            .next()
            .unwrap();
        match bound {
            (Bound::Included(beginning), Bound::Included(end)) => {
                if let ResponseBody::Done(ref body) = *complete_resource.body.lock() {
                    if end == u64::MAX {
                        // Prevent overflow on the addition below.
                        return None;
                    }
                    let b = beginning as usize;
                    let e = end as usize + 1;
                    let requested = body.get(b..e);
                    if let Some(bytes) = requested {
                        let new_resource =
                            create_resource_with_bytes_from_resource(bytes, complete_resource);
                        let cached_headers = new_resource.metadata.headers.lock();
                        let cached_response = create_cached_response(
                            request,
                            &new_resource,
                            &cached_headers,
                            done_chan,
                        );
                        if let Some(cached_response) = cached_response {
                            return Some(cached_response);
                        }
                    }
                }
            },
            (Bound::Included(beginning), Bound::Unbounded) => {
                if let ResponseBody::Done(ref body) = *complete_resource.body.lock() {
                    let b = beginning as usize;
                    let requested = body.get(b..);
                    if let Some(bytes) = requested {
                        let new_resource =
                            create_resource_with_bytes_from_resource(bytes, complete_resource);
                        let cached_headers = new_resource.metadata.headers.lock();
                        let cached_response = create_cached_response(
                            request,
                            &new_resource,
                            &cached_headers,
                            done_chan,
                        );
                        if let Some(cached_response) = cached_response {
                            return Some(cached_response);
                        }
                    }
                }
            },
            _ => return None,
        }
    } else {
        for partial_resource in partial_cached_resources {
            let headers = partial_resource.metadata.headers.lock();
            let content_range = headers.typed_get::<ContentRange>();

            let Some(body_len) = content_range.as_ref().and_then(|range| range.bytes_len()) else {
                continue;
            };
            match range_spec.satisfiable_ranges(body_len - 1).next().unwrap() {
                (Bound::Included(beginning), Bound::Included(end)) => {
                    let (res_beginning, res_end) = match content_range {
                        Some(range) => {
                            if let Some(bytes_range) = range.bytes_range() {
                                bytes_range
                            } else {
                                continue;
                            }
                        },
                        _ => continue,
                    };
                    if res_beginning <= beginning && res_end >= end {
                        let resource_body = &*partial_resource.body.lock();
                        let requested = match resource_body {
                            ResponseBody::Done(body) => {
                                let b = beginning as usize - res_beginning as usize;
                                let e = end as usize - res_beginning as usize + 1;
                                body.get(b..e)
                            },
                            _ => continue,
                        };
                        if let Some(bytes) = requested {
                            let new_resource =
                                create_resource_with_bytes_from_resource(bytes, partial_resource);
                            let cached_response =
                                create_cached_response(request, &new_resource, &headers, done_chan);
                            if let Some(cached_response) = cached_response {
                                return Some(cached_response);
                            }
                        }
                    }
                },

                (Bound::Included(beginning), Bound::Unbounded) => {
                    let (res_beginning, res_end, total) = if let Some(range) = content_range {
                        match (range.bytes_range(), range.bytes_len()) {
                            (Some(bytes_range), Some(total)) => {
                                (bytes_range.0, bytes_range.1, total)
                            },
                            _ => continue,
                        }
                    } else {
                        continue;
                    };
                    if total == 0 {
                        // Prevent overflow in the below operations from occuring.
                        continue;
                    };
                    if res_beginning <= beginning && res_end == total - 1 {
                        let resource_body = &*partial_resource.body.lock();
                        let requested = match resource_body {
                            ResponseBody::Done(body) => {
                                let from_byte = beginning as usize - res_beginning as usize;
                                body.get(from_byte..)
                            },
                            _ => continue,
                        };
                        if let Some(bytes) = requested {
                            if bytes.len() as u64 + beginning < total - 1 {
                                // Requested range goes beyond the available range.
                                continue;
                            }
                            let new_resource =
                                create_resource_with_bytes_from_resource(bytes, partial_resource);
                            let cached_response =
                                create_cached_response(request, &new_resource, &headers, done_chan);
                            if let Some(cached_response) = cached_response {
                                return Some(cached_response);
                            }
                        }
                    }
                },

                _ => continue,
            }
        }
    }

    None
}

/// Constructing Responses from Caches.
/// <https://tools.ietf.org/html/rfc7234#section-4>
pub(crate) fn construct_response(
    request: &Request,
    done_chan: &mut DoneChannel,
    cache_result: &[CachedResource],
) -> Option<CachedResponse> {
    if pref!(network_http_cache_disabled) {
        return None;
    }

    // TODO: generate warning headers as appropriate <https://tools.ietf.org/html/rfc7234#section-5.5>
    debug!("trying to construct cache response for {:?}", request.url());
    if request.method != Method::GET {
        // Only Get requests are cached, avoid a url based match for others.
        debug!("non-GET method, not caching");
        return None;
    }

    let resources = cache_result
        .iter()
        .filter(|r| !r.aborted.load(Ordering::Relaxed));
    let mut candidates = vec![];
    for cached_resource in resources {
        let mut can_be_constructed = true;
        let cached_headers = cached_resource.metadata.headers.lock();
        let original_request_headers = cached_resource.request_headers.lock();
        if let Some(vary_value) = cached_headers.typed_get::<Vary>() {
            if vary_value.is_any() {
                debug!("vary value is any, not caching");
                can_be_constructed = false
            } else {
                // For every header name found in the Vary header of the stored response.
                // Calculating Secondary Keys with Vary <https://tools.ietf.org/html/rfc7234#section-4.1>
                for vary_val in vary_value.iter_strs() {
                    match request.headers.get(vary_val) {
                        Some(header_data) => {
                            // If the header is present in the request.
                            if let Some(original_header_data) =
                                original_request_headers.get(vary_val)
                            {
                                // Check that the value of the nominated header field,
                                // in the original request, matches the value in the current request.
                                if original_header_data != header_data {
                                    debug!("headers don't match, not caching");
                                    can_be_constructed = false;
                                    break;
                                }
                            }
                        },
                        None => {
                            // If a header field is absent from a request,
                            // it can only match a stored response if those headers,
                            // were also absent in the original request.
                            can_be_constructed = original_request_headers.get(vary_val).is_none();
                            if !can_be_constructed {
                                debug!("vary header present, not caching");
                            }
                        },
                    }
                    if !can_be_constructed {
                        break;
                    }
                }
            }
        }
        if can_be_constructed {
            candidates.push(cached_resource);
        }
    }
    // Support for range requests
    if let Some(range_spec) = request.headers.typed_get::<Range>() {
        return handle_range_request(request, candidates.as_slice(), &range_spec, done_chan);
    }
    while let Some(cached_resource) = candidates.pop() {
        // Not a Range request.
        // Do not allow 206 responses to be constructed.
        //
        // See https://tools.ietf.org/html/rfc7234#section-3.1
        //
        // A cache MUST NOT use an incomplete response to answer requests unless the
        // response has been made complete or the request is partial and
        // specifies a range that is wholly within the incomplete response.
        //
        // TODO: Combining partial content to fulfill a non-Range request
        // see https://tools.ietf.org/html/rfc7234#section-3.3
        match cached_resource.status.try_code() {
            Some(ref code) => {
                if *code == StatusCode::PARTIAL_CONTENT {
                    continue;
                }
            },
            None => continue,
        }
        // Returning a response that can be constructed
        // TODO: select the most appropriate one, using a known mechanism from a selecting header field,
        // or using the Date header to return the most recent one.
        let cached_headers = cached_resource.metadata.headers.lock();
        let cached_response =
            create_cached_response(request, cached_resource, &cached_headers, done_chan);
        if let Some(cached_response) = cached_response {
            return Some(cached_response);
        }
    }
    debug!("couldn't find an appropriate response, not caching");
    // The cache wasn't able to construct anything.
    None
}

/// Freshening Stored Responses upon Validation.
/// <https://tools.ietf.org/html/rfc7234#section-4.3.4>
pub fn refresh(
    request: &Request,
    response: Response,
    done_chan: &mut DoneChannel,
    cached_resources: &mut [CachedResource],
) -> Option<Response> {
    assert_eq!(response.status, StatusCode::NOT_MODIFIED);

    let cached_resource = cached_resources.iter_mut().next()?;

    let mut constructed_response = if let Some(range_spec) = request.headers.typed_get::<Range>() {
        handle_range_request(request, &[cached_resource], &range_spec, done_chan)
            .map(|cached_response| cached_response.response)
    } else {
        // done_chan will have been set to Some(..) by http_network_fetch.
        // If the body is not receiving data, set the done_chan back to None.
        // Otherwise, create a new dedicated channel to update the consumer.
        // The response constructed here will replace the 304 one from the network.
        let in_progress_channel = match &*cached_resource.body.lock() {
            ResponseBody::Receiving(..) => Some(unbounded()),
            ResponseBody::Empty | ResponseBody::Done(..) => None,
        };
        match in_progress_channel {
            Some((done_sender, done_receiver)) => {
                *done_chan = Some((done_sender.clone(), done_receiver));
                cached_resource.awaiting_body.lock().push(done_sender);
            },
            None => *done_chan = None,
        }
        // Received a response with 304 status code, in response to a request that matches a cached resource.
        // 1. update the headers of the cached resource.
        // 2. return a response, constructed from the cached resource.
        let resource_timing = ResourceFetchTiming::new(request.timing_type());
        let mut constructed_response =
            Response::new(cached_resource.metadata.final_url.clone(), resource_timing);

        constructed_response.body = cached_resource.body.clone();

        constructed_response
            .status
            .clone_from(&cached_resource.status);
        constructed_response.referrer = request.referrer.to_url().cloned();
        constructed_response.referrer_policy = request.referrer_policy;
        constructed_response
            .status
            .clone_from(&cached_resource.status);
        constructed_response
            .url_list
            .clone_from(&cached_resource.url_list);
        Some(constructed_response)
    };

    // Update cached Resource with response and constructed response.
    if let Some(constructed_response) = constructed_response.as_mut() {
        // Bracket is to minimize lock duration.
        {
            let mut stored_headers = cached_resource.metadata.headers.lock();
            stored_headers.extend(response.headers);
            constructed_response.headers = stored_headers.clone();
        }
        cached_resource.cache_semantics = HttpCacheSemantics::new(constructed_response);
        cached_resource.expires = cached_resource.cache_semantics.freshness_lifetime();
        cached_resource.stale_while_revalidate =
            get_stale_while_revalidate(&constructed_response.headers);
        cached_resource.last_validated = Instant::now();
    }

    constructed_response
}

pub(crate) fn invalidate_cached_resources(cached_resources: &mut [CachedResource]) {
    for cached_resource in cached_resources.iter_mut() {
        cached_resource.expires = Duration::ZERO;
    }
}

fn resolve_location_url(
    request: &Request,
    response: &Response,
    header_name: header::HeaderName,
) -> Option<ServoUrl> {
    response
        .headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .and_then(|location| request.current_url().join(location).ok())
}

fn cache_live_resource(
    meta: StoredVariantMeta,
    body: ResponseBody,
    body_handle: BodyHandle,
) -> CachedResource {
    let resource = meta.into_cached_resource(body_handle);
    *resource.body.lock() = body;
    resource
}

fn stored_meta_from_cached_resource(resource: &CachedResource) -> StoredVariantMeta {
    StoredVariantMeta {
        request_headers: resource.request_headers.lock().clone(),
        response_headers: resource.metadata.headers.lock().clone(),
        final_url: resource.metadata.final_url.clone(),
        content_type: resource.metadata.content_type.clone(),
        charset: resource.metadata.charset.clone(),
        status: resource.status.clone(),
        cache_policy: StoredCachePolicy {
            cacheable: resource.cache_semantics.is_cacheable(),
            freshness_lifetime: resource.cache_semantics.freshness_lifetime(),
        },
        location_url: resource.location_url.clone(),
        body_len: resource.body_len,
        url_list: resource.url_list.clone(),
        expires: resource.expires,
        stale_while_revalidate: resource.stale_while_revalidate,
    }
}

fn update_live_entry_body_state(
    entry: &LiveEntry,
    body: &Arc<ParkingLotMutex<ResponseBody>>,
    body_len: usize,
    aborted: bool,
    resolve_waiters: bool,
) {
    let mut resources = entry.write();
    if let Some(resource) = resources
        .iter_mut()
        .find(|resource| Arc::ptr_eq(&resource.body, body))
    {
        resource.body_len = body_len;
        resource.aborted.store(aborted, Ordering::Release);
        if resolve_waiters {
            let to_send = if aborted { Data::Cancelled } else { Data::Done };
            let mut awaiting_consumers = resource.awaiting_body.lock();
            for done_sender in awaiting_consumers.drain(..) {
                let _ = done_sender.send(to_send.clone());
            }
        }
    }
}

struct LiveEntryBodyWriter {
    entry: LiveEntry,
    body: Arc<ParkingLotMutex<ResponseBody>>,
    body_len: usize,
    body_handle: BodyHandle,
}

impl BodyWriter for LiveEntryBodyWriter {
    fn body_handle(&self) -> BodyHandle {
        self.body_handle.clone()
    }

    fn write(&mut self, chunk: Bytes) -> Result<(), StoreError> {
        let mut body = self.body.try_lock().ok_or(StoreError::Closed)?;
        match &mut *body {
            ResponseBody::Empty => *body = ResponseBody::Receiving(chunk.to_vec()),
            ResponseBody::Receiving(bytes) => bytes.extend_from_slice(&chunk),
            ResponseBody::Done(_) => return Err(StoreError::Closed),
        }
        self.body_len += chunk.len();
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<(), StoreError> {
        let Self {
            entry,
            body,
            body_len,
            ..
        } = *self;
        {
            let mut body_lock = body.lock();
            let finished = match std::mem::replace(&mut *body_lock, ResponseBody::Empty) {
                ResponseBody::Empty => ResponseBody::Done(vec![]),
                ResponseBody::Receiving(bytes) => ResponseBody::Done(bytes),
                ResponseBody::Done(bytes) => ResponseBody::Done(bytes),
            };
            *body_lock = finished;
        }
        update_live_entry_body_state(&entry, &body, body_len, false, true);
        Ok(())
    }

    fn abort(self: Box<Self>) -> Result<(), StoreError> {
        let Self { entry, body, .. } = *self;
        {
            let mut body_lock = body.lock();
            *body_lock = ResponseBody::Empty;
        }
        update_live_entry_body_state(&entry, &body, 0, true, true);
        Ok(())
    }
}

struct MirroredBodyWriter {
    store_writer: Option<Box<dyn BodyWriter>>,
    live_writer: Option<LiveEntryBodyWriter>,
}

impl MirroredBodyWriter {
    fn abort_both(&mut self) {
        if let Some(store_writer) = self.store_writer.take() {
            let _ = store_writer.abort();
        }
        if let Some(live_writer) = self.live_writer.take() {
            let _ = Box::new(live_writer).abort();
        }
    }
}

impl BodyWriter for MirroredBodyWriter {
    fn body_handle(&self) -> BodyHandle {
        self.store_writer
            .as_ref()
            .map(|writer| writer.body_handle())
            .or_else(|| self.live_writer.as_ref().map(|writer| writer.body_handle()))
            .expect("mirrored writer should always have at least one body handle")
    }

    fn write(&mut self, chunk: Bytes) -> Result<(), StoreError> {
        let Some(store_writer) = self.store_writer.as_mut() else {
            return Err(StoreError::Closed);
        };
        if let Err(err) = store_writer.write(chunk.clone()) {
            self.abort_both();
            return Err(err);
        }

        let Some(live_writer) = self.live_writer.as_mut() else {
            self.abort_both();
            return Err(StoreError::Closed);
        };
        if let Err(err) = live_writer.write(chunk) {
            self.abort_both();
            return Err(err);
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<(), StoreError> {
        let Self {
            store_writer,
            live_writer,
        } = *self;
        let store_result = store_writer.map_or(Ok(()), |writer| writer.finish());
        let live_result = live_writer.map_or(Ok(()), |writer| Box::new(writer).finish());
        store_result.and(live_result)
    }

    fn abort(self: Box<Self>) -> Result<(), StoreError> {
        let Self {
            store_writer,
            live_writer,
        } = *self;
        let store_result = store_writer.map_or(Ok(()), |writer| writer.abort());
        let live_result = live_writer.map_or(Ok(()), |writer| Box::new(writer).abort());
        store_result.and(live_result)
    }
}

impl HttpCache {
    fn live_entry(&self, key: &CacheKey) -> Option<LiveEntry> {
        self.live_entries.lock().get(key).cloned()
    }

    /// Return the live entry for `key`, populating it from the backing store
    /// when this process has not seen the key yet.
    ///
    /// The live map only holds what the current process stored, so without this
    /// step a backend that outlives the process — the whole point of a disk
    /// store — would be written but never read: every request would miss,
    /// refetch, and store a duplicate variant.
    async fn live_or_stored_entry(&self, key: &CacheKey) -> Option<LiveEntry> {
        if let Some(entry) = self.live_entry(key) {
            return Some(entry);
        }
        self.hydrate_from_store(key).await
    }

    /// Load every stored variant for `key` into the live map and return it.
    ///
    /// A variant whose body cannot be read is skipped rather than surfaced: the
    /// storage contract is that an unreadable entry behaves as a miss, never as
    /// corrupt bytes.
    async fn hydrate_from_store(&self, key: &CacheKey) -> Option<LiveEntry> {
        let variants = self.store.lookup(key).await;
        let mut entry = None;
        for variant in variants {
            let Ok(mut stream) = self.store.open_body(&variant.body).await else {
                continue;
            };
            let mut bytes = Vec::with_capacity(variant.meta.body_len());
            while let Some(chunk) = stream.next().await {
                bytes.extend_from_slice(&chunk);
            }
            if bytes.len() != variant.meta.body_len() {
                continue;
            }
            let resource =
                cache_live_resource(variant.meta, ResponseBody::Done(bytes), variant.body);
            entry = Some(self.insert_live_entry(key, resource));
        }
        entry
    }

    fn insert_live_entry(&self, key: &CacheKey, resource: CachedResource) -> LiveEntry {
        let mut live_entries = self.live_entries.lock();
        let entry = live_entries
            .entry(key.clone())
            .or_insert_with(|| StdArc::new(ParkingLotRwLock::new(vec![])))
            .clone();
        entry.write().push(resource);
        entry
    }

    /// Wake-up consumers of cached resources
    /// whose response body was still receiving data when the resource was constructed,
    /// and whose response has now either been completed or cancelled.
    pub(crate) async fn update_awaiting_consumers(&self, request: &Request, response: &Response) {
        let entry_key = CacheKey::new(request);

        let Some(entry) = self.live_entry(&entry_key) else {
            return;
        };

        // Enter critical section on cache entry.
        let cached_resources = entry.read();

        let actual_response = response.actual_response();

        // Ensure we only wake-up consumers of relevant resources,
        // ie we don't want to wake-up 200 awaiting consumers with a 206.
        let relevant_cached_resources = cached_resources.iter().filter(|resource| {
            if actual_response.is_network_error() {
                return *resource.body.lock() == ResponseBody::Empty;
            }
            resource.status == actual_response.status
        });

        for cached_resource in relevant_cached_resources {
            let mut awaiting_consumers = cached_resource.awaiting_body.lock();
            if awaiting_consumers.is_empty() {
                continue;
            }
            let to_send = if cached_resource.aborted.load(Ordering::Acquire) {
                // In the case of an aborted fetch, wake up every awaiting
                // consumer so that none is left waiting on a body that will
                // never arrive. Waking all of them no longer risks a request
                // stampede: any fetch that follows enters
                // `http_network_or_cache_fetch`, which takes an in-flight
                // reservation for the key, so at most one of them reaches the
                // network and the others wait on that reservation. Choosing a
                // single winner is the reservation's job, not this one's.
                Data::Cancelled
            } else {
                match *cached_resource.body.lock() {
                    ResponseBody::Done(_) | ResponseBody::Empty => Data::Done,
                    ResponseBody::Receiving(_) => {
                        continue;
                    },
                }
            };
            for done_sender in awaiting_consumers.drain(..) {
                let _ = done_sender.send(to_send.clone());
            }
        }
    }

    /// Returns descriptors for cache entries currently stored in this cache.
    pub(crate) fn cache_entry_descriptors(&self) -> Vec<CacheEntryDescriptor> {
        spawn_blocking_task::<_, ()>(self.store.entries())
    }

    /// Clear the contents of this cache.
    pub(crate) fn clear(&self) {
        spawn_blocking_task::<_, ()>(self.store.clear());
        self.live_entries.lock().clear();
        self.in_flight.lock().clear();
    }

    /// Reserve the cache key for a single producer or wait on the existing one.
    pub fn acquire_inflight(&self, key: CacheKey) -> InFlightReservation<'_> {
        let mut inflight = self.in_flight.lock();
        if let Some(entry) = inflight.get(&key) {
            return InFlightReservation::Waiter(entry.state.subscribe());
        }

        let entry = InFlightEntry::new();
        inflight.insert(key.clone(), entry);
        InFlightReservation::Producer(InFlightLease::new(self, key))
    }

    fn finish_inflight(&self, key: &CacheKey) {
        if let Some(entry) = self.in_flight.lock().remove(key) {
            let _ = entry.state.send(true);
        }
    }

    /// Insert a response for `request` into the cache (used by tests that need direct access).
    pub async fn store(&self, request: &Request, response: &Response) {
        let body = response.body.lock().clone();
        let _ = self.insert_response(request, response, body).await;
    }

    /// Start caching a response whose body will continue streaming.
    pub(crate) async fn start_streaming_entry(
        &self,
        request: &Request,
        response: &Response,
    ) -> Option<Box<dyn BodyWriter>> {
        self.insert_response(request, response, ResponseBody::Receiving(vec![]))
            .await
    }

    async fn insert_response(
        &self,
        request: &Request,
        response: &Response,
        body: ResponseBody,
    ) -> Option<Box<dyn BodyWriter>> {
        if pref!(network_http_cache_disabled) {
            return None;
        }

        if request.method != Method::GET {
            return None;
        }
        if request.headers.contains_key(header::AUTHORIZATION) {
            return None;
        }
        if response.status == StatusCode::NOT_MODIFIED {
            return None;
        }
        let metadata = match response.metadata() {
            Ok(FetchMetadata::Filtered {
                filtered: _,
                unsafe_: metadata,
            }) |
            Ok(FetchMetadata::Unfiltered(metadata)) => metadata,
            _ => return None,
        };
        let cache_semantics = HttpCacheSemantics::new(response);
        if !cache_semantics.is_cacheable() {
            return None;
        }
        let expiry = cache_semantics.freshness_lifetime();
        let stale_while_revalidate = get_stale_while_revalidate(&response.headers);
        let (body_bytes, streaming, live_body) = match body {
            ResponseBody::Empty => (Vec::new(), false, ResponseBody::Empty),
            ResponseBody::Receiving(bytes) => (bytes.clone(), true, ResponseBody::Receiving(bytes)),
            ResponseBody::Done(bytes) => (bytes.clone(), false, ResponseBody::Done(bytes)),
        };

        let mut response_headers = response.headers.clone();
        normalize_cached_response_headers(&mut response_headers);
        let request_headers = sanitized_request_headers(&request.headers);

        let entry_meta = StoredVariantMeta {
            request_headers,
            response_headers,
            final_url: metadata.final_url,
            content_type: metadata.content_type.map(|v| v.0.to_string()),
            charset: metadata.charset,
            status: metadata.status,
            cache_policy: StoredCachePolicy {
                cacheable: cache_semantics.is_cacheable(),
                freshness_lifetime: expiry,
            },
            location_url: response.location_url.clone(),
            body_len: body_bytes.len(),
            url_list: request
                .url_list
                .iter()
                .map(|claimed_url| claimed_url.url())
                .collect(),
            expires: expiry,
            stale_while_revalidate,
        };

        let key = CacheKey::new(request);
        let mut writer = self
            .store
            .start_entry(&key, entry_meta.clone())
            .await
            .ok()?;
        let body_handle = writer.body_handle();
        if !body_bytes.is_empty() {
            if writer.write(Bytes::from(body_bytes.clone())).is_err() {
                let _ = writer.abort();
                return None;
            }
        }

        let live_resource = cache_live_resource(entry_meta, live_body, body_handle.clone());
        if streaming {
            let live_body = live_resource.body.clone();
            let live_entry = self.insert_live_entry(&key, live_resource);
            Some(Box::new(MirroredBodyWriter {
                store_writer: Some(writer),
                live_writer: Some(LiveEntryBodyWriter {
                    entry: live_entry,
                    body: live_body,
                    body_len: body_bytes.len(),
                    body_handle,
                }),
            }))
        } else {
            if writer.finish().is_err() {
                return None;
            }
            let _ = self.insert_live_entry(&key, live_resource);
            None
        }
    }

    /// <https://fetch.spec.whatwg.org/#concept-http-network-or-cache-fetch>
    /// Prepare cache access for a request and resolve any cached response.
    pub(crate) async fn prepare_cache_access<'a>(
        &'a self,
        context: &FetchContext,
        http_request: &mut Request,
        done_chan: &mut DoneChannel,
        revalidating_flag: &mut bool,
        response: &mut Option<Response>,
    ) {
        let entry_key = CacheKey::new(http_request);
        let Some(entry) = self.live_or_stored_entry(&entry_key).await else {
            return;
        };

        let mut cached_resources = entry.write();
        // TODO(#33616): Step 8.23 Set httpCache to the result of determining the
        // HTTP cache partition, given httpRequest.
        // Step 8.25.1 Set storedResponse to the result of selecting a response from the httpCache,
        //              possibly needing validation, as per the "Constructing Responses from Caches"
        //              chapter of HTTP Caching, if any.
        let stored_response =
            construct_response(http_request, done_chan, cached_resources.as_mut_slice());
        // Step 8.25.2 If storedResponse is non-null, then:
        if let Some(response_from_cache) = stored_response {
            let response_headers = response_from_cache.response.headers.clone();
            let validation_status = response_from_cache.validation_status;
            let revalidation_guard = response_from_cache.revalidation_guard.clone();

            // Substep 1, 2, 3, 4
            let (cached_response, needs_synchronous_revalidation) =
                match (http_request.cache_mode, &http_request.mode) {
                    (CacheMode::ForceCache, _) => (Some(response_from_cache.response), false),
                    (CacheMode::OnlyIfCached, &RequestMode::SameOrigin) => {
                        (Some(response_from_cache.response), false)
                    },
                    (CacheMode::OnlyIfCached, _) |
                    (CacheMode::NoStore, _) |
                    (CacheMode::Reload, _) => (None, false),
                    (_, _) => (
                        Some(response_from_cache.response),
                        validation_status ==
                            (ValidationStatus::Stale {
                                revalidate_in_background: false,
                            }),
                    ),
                };

            if needs_synchronous_revalidation {
                *revalidating_flag = true;
                // Substep 5
                if let Some(http_date) = response_headers.typed_get::<LastModified>() {
                    let http_date: SystemTime = http_date.into();
                    http_request
                        .headers
                        .typed_insert(IfModifiedSince::from(http_date));
                }
                if let Some(entity_tag) = response_headers.get(header::ETAG) {
                    http_request
                        .headers
                        .insert(header::IF_NONE_MATCH, entity_tag.clone());
                }
            } else {
                // Substep 6
                // If it's a stale-while-revalidate response, also refresh it in the background.
                let revalidate_in_background = validation_status ==
                    (ValidationStatus::Stale {
                        revalidate_in_background: true,
                    });
                if revalidate_in_background && cached_response.is_some() {
                    spawn_stale_while_revalidate(context, http_request, revalidation_guard);
                }
                *response = cached_response;
                if let Some(response) = response {
                    response.cache_state = CacheState::Local;
                }
            }
            if response.is_none() {
                // Ensure the done chan is not set if we're not using the cached response,
                // as the cache might have set it to Some if it constructed a pending response.
                *done_chan = None;
            }
        }
    }

    /// Try to construct a cached response for `request`.
    pub async fn construct_response(
        &self,
        request: &Request,
        done_chan: &mut DoneChannel,
    ) -> Option<Response> {
        let entry = self.live_or_stored_entry(&CacheKey::new(request)).await?;
        let cached_resources = entry.read();
        construct_response(request, done_chan, cached_resources.as_slice())
            .map(|cached| cached.response)
    }

    /// Like [`construct_response`](Self::construct_response), but additionally
    /// reports the [`ValidationStatus`] of the constructed response.
    #[cfg(feature = "test-util")]
    pub async fn construct_response_freshness(
        &self,
        request: &Request,
        done_chan: &mut DoneChannel,
    ) -> Option<ValidationStatus> {
        let entry = self.live_or_stored_entry(&CacheKey::new(request)).await?;
        let cached_resources = entry.read();
        construct_response(request, done_chan, cached_resources.as_slice())
            .map(|cached| cached.validation_status)
    }

    /// Invalidate cache entries referenced by Location/Content-Location headers.
    pub(crate) async fn invalidate_related_urls(
        &self,
        request: &Request,
        response: &Response,
        skip_key: &CacheKey,
    ) {
        for header_name in &[header::LOCATION, header::CONTENT_LOCATION] {
            if let Some(location_url) = resolve_location_url(request, response, header_name.clone())
            {
                let location_key = CacheKey::from_url(location_url);
                if &location_key != skip_key {
                    self.invalidate_entry(&location_key).await;
                }
            }
        }
    }

    /// Invalidate the live cached entry for `key` in place.
    pub(crate) async fn invalidate_entry(&self, key: &CacheKey) {
        let Some(entry) = self.live_or_stored_entry(key).await else {
            return;
        };

        let mut cached_resources = entry.write();
        invalidate_cached_resources(cached_resources.as_mut_slice());
    }

    /// Refresh a live cached entry in place.
    pub async fn refresh_entry(
        &self,
        request: &Request,
        response: Response,
        done_chan: &mut DoneChannel,
        key: &CacheKey,
    ) -> Option<Response> {
        let entry = self.live_or_stored_entry(key).await?;
        let (refreshed, sync_meta) = {
            let mut cached_resources = entry.write();
            let refreshed = refresh(
                request,
                response,
                done_chan,
                cached_resources.as_mut_slice(),
            );
            let sync_meta = refreshed.as_ref().and_then(|_| {
                cached_resources.first().map(|resource| {
                    (
                        resource.body_handle.clone(),
                        stored_meta_from_cached_resource(resource),
                    )
                })
            });
            (refreshed, sync_meta)
        };

        if let Some((body_handle, meta)) = sync_meta {
            self.store.update_meta(&body_handle, meta).await;
        }

        refreshed
    }
}

impl HttpCache {
    /// Construct a cache with an injected store.
    pub fn with_store(store: Box<dyn HttpCacheStore>) -> Self {
        Self {
            store,
            live_entries: StdArc::new(ParkingLotMutex::new(HashMap::new())),
            in_flight: ParkingLotMutex::new(HashMap::new()),
        }
    }

    #[cfg(any(test, feature = "test-util"))]
    /// Start caching a streaming response in tests.
    #[doc(hidden)]
    pub async fn start_streaming_entry_for_test(
        &self,
        request: &Request,
        response: &Response,
    ) -> Option<Box<dyn BodyWriter>> {
        self.start_streaming_entry(request, response).await
    }

    #[cfg(any(test, feature = "test-util"))]
    /// Query the backing store directly in tests.
    #[cfg_attr(feature = "test-util", allow(dead_code))]
    pub(crate) async fn store_lookup_for_test(&self, key: &CacheKey) -> Vec<StoredVariant> {
        self.store.lookup(key).await
    }
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;
    use http::header::EXPIRES;
    use net_traits::blob_url_store::UrlWithBlobClaim;
    use net_traits::request::{Referrer, RequestBuilder};
    use net_traits::response::{Response, ResponseBody};
    use net_traits::{ResourceFetchTiming, ResourceTimingType};
    use servo_base::id::TEST_PIPELINE_ID;

    use super::*;

    #[tokio::test]
    async fn update_awaiting_consumers_wakes_waiters_after_body_completion() {
        let cache = HttpCache::default();
        let url = ServoUrl::parse("https://servo.org/cache-awaiting-consumers").unwrap();
        let request = RequestBuilder::new(
            None,
            UrlWithBlobClaim::new(url.clone(), None),
            Referrer::NoReferrer,
        )
        .pipeline_id(Some(TEST_PIPELINE_ID))
        .origin(url.origin())
        .build();

        let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
        let mut receiving = Response::new(url.clone(), timing.clone());
        receiving
            .headers
            .insert(EXPIRES, HeaderValue::from_str("-10").unwrap());
        receiving.body = Arc::new(ParkingLotMutex::new(ResponseBody::Receiving(vec![])));

        cache.store(&request, &receiving).await;

        let mut done_chan = None;
        let cached = cache
            .construct_response(&request, &mut done_chan)
            .await
            .expect("cached response should be constructed");
        assert!(matches!(*cached.body.lock(), ResponseBody::Receiving(_)));
        let (sender, mut receiver) =
            done_chan.expect("construct_response should attach a done channel");

        let mut finished = Response::new(url.clone(), timing);
        finished
            .headers
            .insert(EXPIRES, HeaderValue::from_str("-10").unwrap());
        finished.body = Arc::new(ParkingLotMutex::new(ResponseBody::Done(vec![])));

        cache.update_awaiting_consumers(&request, &finished).await;

        let _ = sender;
        assert!(receiver.try_recv().is_ok());
    }
}
