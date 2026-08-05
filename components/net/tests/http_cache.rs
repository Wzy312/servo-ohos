/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::fs;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, EXPIRES, HeaderValue, RANGE};
use http::{HeaderMap, StatusCode};
use malloc_size_of::{MallocSizeOf, MallocSizeOfOps};
use net::http_cache::{CacheKey, HttpCache, InFlightReservation, ValidationStatus};
use net::test::{
    DiskStore, HttpCacheStore, MemoryStore, StoredVariantMeta, build_stored_variant_meta,
    collect_body,
};
use net_traits::blob_url_store::UrlWithBlobClaim;
use net_traits::request::{CacheMode, Referrer, RequestBuilder};
use net_traits::response::{Response, ResponseBody};
use net_traits::{ResourceFetchTiming, ResourceTimingType};
use parking_lot::Mutex;
use servo_base::id::TEST_PIPELINE_ID;
use servo_url::ServoUrl;
use tempfile::tempdir;
use tokio::sync::mpsc::unbounded_channel as unbounded;

unsafe extern "C" fn zero_usable_size(_: *const std::ffi::c_void) -> usize {
    0
}

#[tokio::test]
async fn test_refreshing_resource_sets_done_chan_the_appropriate_value() {
    let response_bodies = vec![
        ResponseBody::Receiving(vec![]),
        ResponseBody::Empty,
        ResponseBody::Done(vec![]),
    ];
    let url = ServoUrl::parse("https://servo.org").unwrap();
    let request = RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .pipeline_id(Some(TEST_PIPELINE_ID))
    .origin(url.origin())
    .build();
    for body in response_bodies {
        let cache = HttpCache::default();
        let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
        let mut stored_response = Response::new(url.clone(), timing);
        // Expires header makes the response cacheable.
        stored_response
            .headers
            .insert(EXPIRES, HeaderValue::from_str("-10").unwrap());
        *stored_response.body.lock() = body.clone();
        cache.store(&request, &stored_response).await;

        let mut response = Response::new(
            url.clone(),
            ResourceFetchTiming::new(ResourceTimingType::Navigation),
        );
        response.status = StatusCode::NOT_MODIFIED.into();
        let (send, recv) = unbounded();
        let mut done_chan = Some((send, recv));
        let refreshed_response = cache
            .refresh_entry(&request, response, &mut done_chan, &CacheKey::new(&request))
            .await;
        // Ensure a resource was found, and refreshed.
        assert!(refreshed_response.is_some());
        match body {
            ResponseBody::Receiving(_) => assert!(done_chan.is_some()),
            ResponseBody::Empty | ResponseBody::Done(_) => assert!(done_chan.is_none()),
        }
    }
}

#[tokio::test]
async fn cache_constructs_and_refreshes_from_http_cache_live_state() {
    let store = MemoryStore::default();
    let cache = HttpCache::with_store(Box::new(store.clone()));
    let url = ServoUrl::parse("https://servo.org/cache-live-state").unwrap();
    let request = RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .pipeline_id(Some(TEST_PIPELINE_ID))
    .origin(url.origin())
    .build();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url.clone(), timing);
    response
        .headers
        .insert(EXPIRES, HeaderValue::from_str("-10").unwrap());
    response.body = servo_arc::Arc::new(Mutex::new(ResponseBody::Done(b"live".to_vec())));

    cache.store(&request, &response).await;

    let stored_before = store.lookup(&CacheKey::new(&request)).await;
    assert_eq!(stored_before.len(), 1);
    assert_eq!(stored_before[0].body_len(), 4);

    let mut done_chan = None;
    let cached = cache
        .construct_response(&request, &mut done_chan)
        .await
        .expect("cached response should be constructed from the live cache state");
    assert!(matches!(*cached.body.lock(), ResponseBody::Done(ref body) if body == b"live"));

    let mut not_modified = Response::new(
        url.clone(),
        ResourceFetchTiming::new(ResourceTimingType::Navigation),
    );
    not_modified.status = StatusCode::NOT_MODIFIED.into();
    not_modified.headers.insert(
        http::header::HeaderName::from_static("x-refresh"),
        HeaderValue::from_static("updated"),
    );

    let refreshed = cache
        .refresh_entry(
            &request,
            not_modified,
            &mut done_chan,
            &CacheKey::new(&request),
        )
        .await
        .expect("refresh should use the HTTP cache live state");
    assert_eq!(refreshed.status, StatusCode::OK);

    let stored_after = store.lookup(&CacheKey::new(&request)).await;
    assert_eq!(stored_after.len(), 1);
    assert_eq!(
        stored_after[0]
            .response_headers()
            .get(http::header::HeaderName::from_static("x-refresh"))
            .and_then(|value| value.to_str().ok()),
        Some("updated")
    );
}

#[tokio::test]
async fn test_inflight_requests_are_single_flight_per_key() {
    let cache = HttpCache::default();
    let key = CacheKey::from_url(ServoUrl::parse("https://servo.org").unwrap());

    let first = cache.acquire_inflight(key.clone());
    assert!(matches!(&first, InFlightReservation::Producer(_)));

    let second = cache.acquire_inflight(key);
    assert!(matches!(&second, InFlightReservation::Waiter(_)));

    let mut waiter = Box::pin(second.wait());
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut waiter)
            .await
            .is_err()
    );

    drop(first);
    tokio::time::timeout(Duration::from_secs(1), &mut waiter)
        .await
        .expect("waiter should be released when the producer finishes");
}

#[tokio::test]
async fn test_http_cache_size_accounts_for_inflight_reservations() {
    let cache = HttpCache::default();
    let key = CacheKey::from_url(ServoUrl::parse("https://servo.org/cache-size").unwrap());

    let mut before_ops = MallocSizeOfOps::new(zero_usable_size, None, Some(Box::new(|_| false)));
    let before = cache.size_of(&mut before_ops);

    let inflight = cache.acquire_inflight(key);

    let mut after_ops = MallocSizeOfOps::new(zero_usable_size, None, Some(Box::new(|_| false)));
    let after = cache.size_of(&mut after_ops);

    assert!(after > before);
    drop(inflight);
}

#[tokio::test]
async fn test_skip_incomplete_cache_for_range_request_with_no_end_bound() {
    let actual_body_len = 10;
    let incomplete_response_body = &[1, 2, 3, 4, 5];
    let url = ServoUrl::parse("https://servo.org").unwrap();

    let cache = HttpCache::default();
    let mut headers = HeaderMap::new();

    headers.insert(
        RANGE,
        HeaderValue::from_str(&format!("bytes={}-", 0)).unwrap(),
    );
    let request = RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .pipeline_id(Some(TEST_PIPELINE_ID))
    .origin(url.origin())
    .headers(headers)
    .build();

    // Store incomplete response to http_cache
    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut initial_incomplete_response = Response::new(url.clone(), timing);
    *initial_incomplete_response.body.lock() =
        ResponseBody::Done(incomplete_response_body.to_vec());
    initial_incomplete_response.headers.insert(
        CONTENT_RANGE,
        HeaderValue::from_str(&format!(
            "bytes 0-{}/{}",
            actual_body_len - 1,
            actual_body_len
        ))
        .unwrap(),
    );
    initial_incomplete_response.headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&format!("{}", actual_body_len)).unwrap(),
    );
    initial_incomplete_response
        .headers
        .insert(EXPIRES, HeaderValue::from_str("0").unwrap());
    initial_incomplete_response.status = StatusCode::PARTIAL_CONTENT.into();
    cache.store(&request, &initial_incomplete_response).await;

    // Try to construct response from http_cache
    let mut done_chan = None;
    let consecutive_response = cache.construct_response(&request, &mut done_chan).await;
    assert!(
        consecutive_response.is_none(),
        "Should not construct response from incomplete response!"
    );
}

fn build_stale_while_revalidate_test_request() -> net_traits::request::Request {
    build_stale_while_revalidate_test_request_with_headers(HeaderMap::new())
}

fn build_stale_while_revalidate_test_request_with_headers(
    headers: HeaderMap,
) -> net_traits::request::Request {
    let url = ServoUrl::parse("https://servo.org").unwrap();
    RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .pipeline_id(Some(TEST_PIPELINE_ID))
    .origin(url.origin())
    .headers(headers)
    .build()
}

/// Store a response with the given `Cache-Control` value, then return the
/// freshness state [`ValidationStatus`] reported by the cache for a subsequent request.
async fn stale_while_revalidate_freshness_for_cache_control(
    cache_control: &str,
) -> ValidationStatus {
    let url = ServoUrl::parse("https://servo.org").unwrap();
    let request = build_stale_while_revalidate_test_request();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url, timing);
    *response.body.lock() = ResponseBody::Done(vec![1, 2, 3]);
    response
        .headers
        .insert(CACHE_CONTROL, HeaderValue::from_str(cache_control).unwrap());

    let cache = HttpCache::default();
    cache.store(&request, &response).await;

    let mut done_chan = None;
    cache
        .construct_response_freshness(&request, &mut done_chan)
        .await
        .expect("a response should be constructable from the cache")
}

#[tokio::test]
async fn test_stale_within_stale_while_revalidate_window_serves_immediately_and_revalidates_in_background()
 {
    let validation_status =
        stale_while_revalidate_freshness_for_cache_control("max-age=0, stale-while-revalidate=30")
            .await;
    assert_eq!(
        validation_status,
        ValidationStatus::Stale {
            revalidate_in_background: true
        },
        "stale response within the stale-while-revalidate window should be served immediately \
         and revalidated in the background"
    );
}

#[tokio::test]
async fn test_stale_without_stale_while_revalidate_requires_synchronous_validation() {
    let validation_status = stale_while_revalidate_freshness_for_cache_control("max-age=0").await;
    assert_eq!(
        validation_status,
        ValidationStatus::Stale {
            revalidate_in_background: false
        },
        "stale response without stale-while-revalidate must be synchronously revalidated"
    );
}

#[tokio::test]
async fn test_private_response_with_stale_while_revalidate_is_cached() {
    let url = ServoUrl::parse("https://servo.org").unwrap();
    let request = build_stale_while_revalidate_test_request();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url, timing);
    *response.body.lock() = ResponseBody::Done(vec![1, 2, 3]);
    response.headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_str("private, max-age=0, stale-while-revalidate=30").unwrap(),
    );

    let cache = HttpCache::default();
    cache.store(&request, &response).await;

    let mut done_chan = None;
    let validation_status = cache
        .construct_response_freshness(&request, &mut done_chan)
        .await
        .expect("a response should be constructable from the cache");

    assert_eq!(
        validation_status,
        ValidationStatus::Stale {
            revalidate_in_background: true
        },
        "private responses with stale-while-revalidate should still be cached"
    );
}

#[tokio::test]
async fn test_stale_while_revalidate_not_used_when_request_demands_revalidation() {
    let url = ServoUrl::parse("https://servo.org").unwrap();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url.clone(), timing);
    *response.body.lock() = ResponseBody::Done(vec![1, 2, 3]);
    response.headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_str("max-age=0, stale-while-revalidate=30").unwrap(),
    );

    let store_request = build_stale_while_revalidate_test_request();
    let cache = HttpCache::default();
    cache.store(&store_request, &response).await;

    let mut req_headers = HeaderMap::new();
    req_headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    let request = RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .pipeline_id(Some(TEST_PIPELINE_ID))
    .origin(url.origin())
    .headers(req_headers)
    .build();

    let mut done_chan = None;
    let validation_status = cache
        .construct_response_freshness(&request, &mut done_chan)
        .await
        .expect("a response should be constructable from the cache");
    assert_eq!(
        validation_status,
        ValidationStatus::Stale {
            revalidate_in_background: false
        },
        "a no-cache request must trigger synchronous validation, not background revalidation"
    );
}

#[tokio::test]
async fn test_stale_while_revalidate_not_used_when_request_has_split_cache_control_headers() {
    let url = ServoUrl::parse("https://servo.org").unwrap();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url.clone(), timing);
    *response.body.lock() = ResponseBody::Done(vec![1, 2, 3]);
    response.headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_str("max-age=0, stale-while-revalidate=30").unwrap(),
    );

    let store_request = build_stale_while_revalidate_test_request();
    let cache = HttpCache::default();
    cache.store(&store_request, &response).await;

    let mut req_headers = HeaderMap::new();
    req_headers.append(
        CACHE_CONTROL,
        HeaderValue::from_static("stale-while-revalidate=30"),
    );
    req_headers.append(CACHE_CONTROL, HeaderValue::from_static("max-age=000"));
    let request = build_stale_while_revalidate_test_request_with_headers(req_headers);

    let mut done_chan = None;
    let validation_status = cache
        .construct_response_freshness(&request, &mut done_chan)
        .await
        .expect("a response should be constructable from the cache");
    assert_eq!(
        validation_status,
        ValidationStatus::Stale {
            revalidate_in_background: false
        },
        "split cache-control headers should still demand synchronous validation"
    );
}

#[tokio::test]
async fn test_stale_while_revalidate_not_used_when_request_cache_mode_is_no_store() {
    let url = ServoUrl::parse("https://servo.org").unwrap();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url.clone(), timing);
    *response.body.lock() = ResponseBody::Done(vec![1, 2, 3]);
    response.headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_str("max-age=0, stale-while-revalidate=30").unwrap(),
    );

    let store_request = build_stale_while_revalidate_test_request();
    let cache = HttpCache::default();
    cache.store(&store_request, &response).await;

    let request = RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .pipeline_id(Some(TEST_PIPELINE_ID))
    .origin(url.origin())
    .cache_mode(CacheMode::NoStore)
    .build();

    let mut done_chan = None;
    let validation_status = cache
        .construct_response_freshness(&request, &mut done_chan)
        .await
        .expect("a response should be constructable from the cache");
    assert_eq!(
        validation_status,
        ValidationStatus::Stale {
            revalidate_in_background: false
        },
        "a no-store request must trigger synchronous validation, not background revalidation"
    );
}

#[tokio::test]
async fn test_no_store_response_with_max_age_is_not_cached() {
    let url = ServoUrl::parse("https://servo.org").unwrap();
    let request = build_stale_while_revalidate_test_request();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url, timing);
    *response.body.lock() = ResponseBody::Done(vec![1, 2, 3]);
    response.headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_str("no-store, max-age=60").unwrap(),
    );

    let cache = HttpCache::default();
    cache.store(&request, &response).await;

    let mut done_chan = None;
    assert!(
        cache
            .construct_response_freshness(&request, &mut done_chan)
            .await
            .is_none(),
        "no-store responses must never be stored, even when they advertise max-age"
    );
}

async fn memory_store_contract_covers_lookup_insert_completion_and_enumeration_impl<
    S: HttpCacheStore,
>(
    store: &S,
) {
    let url = ServoUrl::parse("https://servo.org/cache-contract").unwrap();
    let key = CacheKey::from_url(url.clone());
    let meta = build_stored_variant_meta(&url, 0);

    assert!(HttpCacheStore::lookup(store, &key).await.is_empty());
    assert!(HttpCacheStore::entries(store).await.is_empty());

    let mut writer = HttpCacheStore::start_entry(store, &key, meta.clone())
        .await
        .expect("entry should be created");
    writer
        .write(bytes::Bytes::from_static(b"he"))
        .expect("first write should succeed");
    writer
        .write(bytes::Bytes::from_static(b"llo"))
        .expect("second write should succeed");

    writer.finish().expect("finishing the body should succeed");

    let stored = HttpCacheStore::lookup(store, &key).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].body_len(), 5);

    let descriptors = HttpCacheStore::entries(store).await;
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].key, url.to_string());
}

#[tokio::test]
async fn memory_store_contract_covers_lookup_insert_body_completion_and_metadata_updates() {
    let store = MemoryStore::default();
    memory_store_contract_covers_lookup_insert_completion_and_enumeration_impl(&store).await;
}

async fn memory_store_contract_abort_discards_partial_body_impl<S: HttpCacheStore>(store: &S) {
    let url = ServoUrl::parse("https://servo.org/cache-contract-abort").unwrap();
    let key = CacheKey::from_url(url.clone());
    let meta = build_stored_variant_meta(&url, 0);

    let mut writer = HttpCacheStore::start_entry(store, &key, meta)
        .await
        .expect("entry should be created");
    writer
        .write(bytes::Bytes::from_static(b"partial"))
        .expect("first write should succeed");
    writer.abort().expect("aborting the body should succeed");

    let stored = HttpCacheStore::lookup(store, &key).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].body_len(), 0);
}

#[tokio::test]
async fn memory_store_contract_abort_discards_partial_body() {
    let store = MemoryStore::default();
    memory_store_contract_abort_discards_partial_body_impl(&store).await;
}

#[tokio::test]
async fn disk_store_contract_abort_discards_partial_body() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    let url = ServoUrl::parse("https://servo.org/cache-contract-abort").unwrap();
    let key = CacheKey::from_url(url.clone());
    let meta = build_stored_variant_meta(&url, 0);

    let mut writer = HttpCacheStore::start_entry(&store, &key, meta)
        .await
        .expect("entry should be created");
    writer
        .write(bytes::Bytes::from_static(b"partial"))
        .expect("first write should succeed");
    writer.abort().expect("aborting the body should succeed");

    assert!(HttpCacheStore::lookup(&store, &key).await.is_empty());
    let entry_files = std::fs::read_dir(temp_dir.path())
        .expect("disk cache directory should still exist")
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("entry"))
        .count();
    assert_eq!(entry_files, 0);
}

#[tokio::test]
async fn disk_store_contract_uses_stored_length_not_live_body_state() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    let url = ServoUrl::parse("https://servo.org/cache-stable-weight").unwrap();
    let key = CacheKey::from_url(url.clone());
    let mut writer = HttpCacheStore::start_entry(&store, &key, build_stored_variant_meta(&url, 0))
        .await
        .expect("entry should be created");
    writer
        .write(bytes::Bytes::from_static(b"defg"))
        .expect("body write should succeed");
    writer.finish().expect("body finish should succeed");

    let stored = HttpCacheStore::lookup(&store, &key).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].body_len(), 4);

    let rebuilt = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    let reloaded = HttpCacheStore::lookup(&rebuilt, &key).await;
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].body_len(), 4);
}

async fn memory_store_contract_replaces_same_variant_instead_of_accumulating_duplicates_impl<
    S: HttpCacheStore,
>(
    store: &S,
) {
    let url = ServoUrl::parse("https://servo.org/cache-contract-replace").unwrap();
    let key = CacheKey::from_url(url.clone());

    let mut first_writer =
        HttpCacheStore::start_entry(store, &key, build_stored_variant_meta(&url, 0))
            .await
            .expect("first entry should be created");
    first_writer
        .write(bytes::Bytes::from_static(b"old"))
        .expect("first body write should succeed");
    first_writer
        .finish()
        .expect("first body finish should succeed");

    let mut second_writer =
        HttpCacheStore::start_entry(store, &key, build_stored_variant_meta(&url, 0))
            .await
            .expect("second entry should be created");
    second_writer
        .write(bytes::Bytes::from_static(b"new"))
        .expect("second body write should succeed");
    second_writer
        .finish()
        .expect("second body finish should succeed");

    // The name of this test says replace, and RFC 9111 agrees: with no `Vary`
    // on the stored response there is exactly one variant per key. It asserted
    // two until the accumulation it describes was actually fixed.
    let stored = HttpCacheStore::lookup(store, &key).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(
        collect_body(store, &stored[0].body).await,
        Bytes::from_static(b"new")
    );
}

#[tokio::test]
async fn memory_store_contract_replaces_same_variant_instead_of_accumulating_duplicates() {
    let store = MemoryStore::default();
    memory_store_contract_replaces_same_variant_instead_of_accumulating_duplicates_impl(&store)
        .await;
}

#[tokio::test]
async fn disk_store_contract_replaces_same_variant_instead_of_accumulating_duplicates() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    memory_store_contract_replaces_same_variant_instead_of_accumulating_duplicates_impl(&store)
        .await;
}

#[tokio::test]
async fn cache_construct_response_strips_content_encoding_for_decoded_bodies() {
    let cache = HttpCache::default();
    let url = ServoUrl::parse("https://servo.org/cache-content-encoding").unwrap();
    let request = RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .pipeline_id(Some(TEST_PIPELINE_ID))
    .origin(url.origin())
    .build();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url.clone(), timing);
    response.headers.insert(
        http::header::CACHE_CONTROL,
        HeaderValue::from_static("max-age=100"),
    );
    response.headers.insert(
        http::header::CONTENT_ENCODING,
        HeaderValue::from_static("gzip"),
    );
    response
        .headers
        .insert(http::header::CONTENT_LENGTH, HeaderValue::from_static("5"));
    response.body = servo_arc::Arc::new(Mutex::new(ResponseBody::Done(b"hello".to_vec())));

    cache.store(&request, &response).await;

    let mut done_chan = None;
    let cached = cache
        .construct_response(&request, &mut done_chan)
        .await
        .expect("cached response should be constructed");

    assert!(cached.headers.get(http::header::CONTENT_ENCODING).is_none());
    assert!(cached.headers.get(http::header::CONTENT_LENGTH).is_none());
}

#[tokio::test]
async fn cache_construct_response_skips_in_progress_bodies() {
    let cache = HttpCache::default();
    let url = ServoUrl::parse("https://servo.org/cache-in-progress").unwrap();
    let request = RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .pipeline_id(Some(TEST_PIPELINE_ID))
    .origin(url.origin())
    .build();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url.clone(), timing);
    response
        .headers
        .insert(EXPIRES, HeaderValue::from_str("-10").unwrap());
    response.body = servo_arc::Arc::new(Mutex::new(ResponseBody::Receiving(vec![])));

    let mut writer = cache
        .start_streaming_entry_for_test(&request, &response)
        .await
        .expect("streaming cache entry should be created");

    let mut done_chan = None;
    assert!(
        cache
            .construct_response(&request, &mut done_chan)
            .await
            .is_none()
    );
    assert!(done_chan.is_none());

    writer
        .write(Bytes::from_static(b"hello"))
        .expect("streaming body write should succeed");
    writer
        .finish()
        .expect("streaming body finish should succeed");

    let cached = cache
        .construct_response(&request, &mut done_chan)
        .await
        .expect("finished body should become cacheable");
    assert!(matches!(*cached.body.lock(), ResponseBody::Done(ref body) if body == b"hello"));
}

async fn memory_store_contract_covers_remove_clear_and_entry_enumeration_impl<S: HttpCacheStore>(
    store: &S,
) {
    let url_a = ServoUrl::parse("https://servo.org/cache-a").unwrap();
    let url_b = ServoUrl::parse("https://servo.org/cache-b").unwrap();
    let key_a = CacheKey::from_url(url_a.clone());
    let key_b = CacheKey::from_url(url_b.clone());

    let mut writer_a =
        HttpCacheStore::start_entry(store, &key_a, build_stored_variant_meta(&url_a, 0))
            .await
            .expect("first entry should be created");
    writer_a
        .write(bytes::Bytes::from_static(b"a"))
        .expect("body write should succeed");
    writer_a.finish().expect("body finish should succeed");

    let mut writer_b =
        HttpCacheStore::start_entry(store, &key_b, build_stored_variant_meta(&url_b, 0))
            .await
            .expect("second entry should be created");
    writer_b
        .write(bytes::Bytes::from_static(b"b"))
        .expect("body write should succeed");
    writer_b.finish().expect("body finish should succeed");

    let mut descriptors = HttpCacheStore::entries(store).await;
    descriptors.sort_by(|left, right| left.key.cmp(&right.key));
    assert_eq!(
        descriptors
            .iter()
            .map(|descriptor| descriptor.key.clone())
            .collect::<Vec<_>>(),
        vec![url_a.to_string(), url_b.to_string()]
    );

    HttpCacheStore::remove(store, &key_a).await;
    assert!(HttpCacheStore::lookup(store, &key_a).await.is_empty());

    assert_eq!(HttpCacheStore::lookup(store, &key_b).await.len(), 1);

    HttpCacheStore::clear(store).await;
    assert!(HttpCacheStore::lookup(store, &key_a).await.is_empty());
    assert!(HttpCacheStore::lookup(store, &key_b).await.is_empty());
    assert!(HttpCacheStore::entries(store).await.is_empty());
}

async fn store_contract_covers_lookup_insert_completion_and_enumeration_impl<S: HttpCacheStore>(
    store: &S,
) {
    let url = ServoUrl::parse("https://servo.org/disk-contract").unwrap();
    let key = CacheKey::from_url(url.clone());

    assert!(HttpCacheStore::lookup(store, &key).await.is_empty());

    let mut writer = HttpCacheStore::start_entry(store, &key, build_stored_variant_meta(&url, 0))
        .await
        .expect("disk entry should be created");
    writer
        .write(bytes::Bytes::from_static(b"hello"))
        .expect("body write should succeed");
    let body_handle = writer.body_handle();
    writer.finish().expect("body finish should succeed");

    let stored = HttpCacheStore::lookup(store, &key).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].body_len(), 5);

    let mut body = HttpCacheStore::open_body(store, &body_handle)
        .await
        .expect("open_body should succeed");
    assert_eq!(
        body.next()
            .await
            .expect("body stream should yield one chunk"),
        Bytes::from_static(b"hello")
    );

    let descriptors = HttpCacheStore::entries(store).await;
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].key, url.to_string());
}

async fn disk_store_contract_covers_lookup_insert_completion_and_enumeration_impl<
    S: HttpCacheStore,
>(
    store: &S,
) {
    store_contract_covers_lookup_insert_completion_and_enumeration_impl(store).await;
}

async fn disk_store_contract_update_meta_remove_clear_impl<S: HttpCacheStore>(store: &S) {
    let url = ServoUrl::parse("https://servo.org/disk-update-meta").unwrap();
    let key = CacheKey::from_url(url.clone());

    let mut writer = HttpCacheStore::start_entry(store, &key, build_stored_variant_meta(&url, 0))
        .await
        .expect("disk entry should be created");
    let body_handle = writer.body_handle();
    writer
        .write(bytes::Bytes::from_static(b"abc"))
        .expect("body write should succeed");
    writer.finish().expect("body finish should succeed");

    let mut updated = build_stored_variant_meta(&url, 0);
    let updated_url = ServoUrl::parse("https://servo.org/disk-update-meta/updated").unwrap();
    updated.set_final_url(updated_url.clone());
    updated.set_status(StatusCode::ACCEPTED.into());
    HttpCacheStore::update_meta(store, &body_handle, updated.clone()).await;

    let stored = HttpCacheStore::lookup(store, &key).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].final_url(), updated_url);
    assert_eq!(stored[0].status(), StatusCode::ACCEPTED);

    HttpCacheStore::remove(store, &key).await;
    assert!(HttpCacheStore::lookup(store, &key).await.is_empty());

    let key2 = CacheKey::from_url(ServoUrl::parse("https://servo.org/disk-clear").unwrap());
    let writer2 =
        HttpCacheStore::start_entry(store, &key2, build_stored_variant_meta(key2.url(), 0))
            .await
            .expect("disk entry should be created");
    writer2.finish().expect("empty body should finish cleanly");
    HttpCacheStore::clear(store).await;
    assert!(HttpCacheStore::entries(store).await.is_empty());
}

async fn disk_store_contract_corrupt_entry_is_a_miss_impl<S: HttpCacheStore>(store: &S) {
    let url = ServoUrl::parse("https://servo.org/disk-corrupt").unwrap();
    let key = CacheKey::from_url(url.clone());
    let mut writer = HttpCacheStore::start_entry(store, &key, build_stored_variant_meta(&url, 0))
        .await
        .expect("disk entry should be created");
    let body_handle = writer.body_handle();
    writer
        .write(bytes::Bytes::from_static(b"body"))
        .expect("body write should succeed");
    writer.finish().expect("body finish should succeed");

    if let Some(path) = body_handle.path() {
        let mut bytes = fs::read(path).expect("entry file should exist");
        bytes[0] ^= 0xff;
        fs::write(path, bytes).expect("entry file should be corruptible");
    }

    assert!(HttpCacheStore::lookup(store, &key).await.is_empty());
}

async fn disk_store_contract_deleted_directory_rebuilds_impl<S: HttpCacheStore>(
    store: &S,
    root: &std::path::Path,
) {
    let url = ServoUrl::parse("https://servo.org/disk-deleted-dir").unwrap();
    let key = CacheKey::from_url(url.clone());
    let writer = HttpCacheStore::start_entry(store, &key, build_stored_variant_meta(&url, 0))
        .await
        .expect("disk entry should be created");
    writer.finish().expect("empty body should finish cleanly");

    fs::remove_dir_all(root).expect("disk cache directory should be removable");
    assert!(HttpCacheStore::lookup(store, &key).await.is_empty());
}

async fn disk_store_contract_partial_deletion_rebuilds_impl<S: HttpCacheStore>(
    store: &S,
    root: &std::path::Path,
) {
    let url = ServoUrl::parse("https://servo.org/disk-partial-delete").unwrap();
    let key = CacheKey::from_url(url.clone());
    let mut writer = HttpCacheStore::start_entry(store, &key, build_stored_variant_meta(&url, 0))
        .await
        .expect("disk entry should be created");
    let body_handle = writer.body_handle();
    writer
        .write(bytes::Bytes::from_static(b"partial"))
        .expect("body write should succeed");
    writer.finish().expect("body finish should succeed");

    if let Some(path) = body_handle.path() {
        fs::remove_file(path).expect("body file should be removable");
    }
    assert!(HttpCacheStore::lookup(store, &key).await.is_empty());

    let _ = fs::create_dir_all(root);
}

#[tokio::test]
async fn stored_variant_meta_round_trips_and_restores_from_json() {
    let url = ServoUrl::parse("https://servo.org/cache-roundtrip").unwrap();
    let original = build_stored_variant_meta(&url, 0);
    let encoded = serde_json::to_string(&original).expect("metadata should serialize");
    let decoded: StoredVariantMeta =
        serde_json::from_str(&encoded).expect("metadata should deserialize");
    assert_eq!(decoded, original);

    let store = MemoryStore::default();
    let key = CacheKey::from_url(url.clone());
    let writer = HttpCacheStore::start_entry(&store, &key, decoded.clone())
        .await
        .expect("entry should be created from stored metadata");
    writer.finish().expect("empty body should finish cleanly");

    let stored = HttpCacheStore::lookup(&store, &key).await;
    assert_eq!(
        stored.iter().map(|v| v.meta.clone()).collect::<Vec<_>>(),
        vec![decoded]
    );
}

#[tokio::test]
async fn memory_store_contract_covers_remove_clear_and_entry_enumeration() {
    let store = MemoryStore::default();
    memory_store_contract_covers_remove_clear_and_entry_enumeration_impl(&store).await;
}

#[tokio::test]
async fn memory_store_contract_covers_lookup_insert_completion_and_enumeration() {
    let store = MemoryStore::default();
    store_contract_covers_lookup_insert_completion_and_enumeration_impl(&store).await;
}

#[tokio::test]
async fn disk_store_contract_covers_lookup_insert_completion_and_enumeration() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    disk_store_contract_covers_lookup_insert_completion_and_enumeration_impl(&store).await;
}

#[tokio::test]
async fn disk_store_contract_update_meta_remove_clear() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    disk_store_contract_update_meta_remove_clear_impl(&store).await;
}

#[tokio::test]
async fn disk_store_contract_corrupt_entry_is_a_miss() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    disk_store_contract_corrupt_entry_is_a_miss_impl(&store).await;
}

#[tokio::test]
async fn disk_store_contract_deleted_directory_rebuilds() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    disk_store_contract_deleted_directory_rebuilds_impl(&store, temp_dir.path()).await;
}

#[tokio::test]
async fn disk_store_contract_partial_deletion_rebuilds() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    disk_store_contract_partial_deletion_rebuilds_impl(&store, temp_dir.path()).await;
}

#[tokio::test]
async fn disk_store_contract_corrupt_index_rebuilds() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    let url = ServoUrl::parse("https://servo.org/disk-index-corrupt").unwrap();
    let key = CacheKey::from_url(url.clone());
    let writer = HttpCacheStore::start_entry(&store, &key, build_stored_variant_meta(&url, 0))
        .await
        .expect("disk entry should be created");
    writer.finish().expect("empty body should finish cleanly");

    fs::write(temp_dir.path().join("index.json"), b"not-json")
        .expect("index should be corruptible");
    let rebuilt = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    assert_eq!(HttpCacheStore::lookup(&rebuilt, &key).await.len(), 1);
}

#[tokio::test]
async fn disk_store_contract_evicts_least_recently_used_entries() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8);

    let url_a = ServoUrl::parse("https://servo.org/disk-evict-a").unwrap();
    let key_a = CacheKey::from_url(url_a.clone());
    let mut writer_a =
        HttpCacheStore::start_entry(&store, &key_a, build_stored_variant_meta(&url_a, 0))
            .await
            .expect("first entry should be created");
    writer_a
        .write(bytes::Bytes::from_static(b"aaaa"))
        .expect("body write should succeed");
    writer_a.finish().expect("body finish should succeed");

    let url_b = ServoUrl::parse("https://servo.org/disk-evict-b").unwrap();
    let key_b = CacheKey::from_url(url_b.clone());
    let mut writer_b =
        HttpCacheStore::start_entry(&store, &key_b, build_stored_variant_meta(&url_b, 0))
            .await
            .expect("second entry should be created");
    writer_b
        .write(bytes::Bytes::from_static(b"bbbb"))
        .expect("body write should succeed");
    writer_b.finish().expect("body finish should succeed");

    let _ = HttpCacheStore::lookup(&store, &key_a).await;

    let url_c = ServoUrl::parse("https://servo.org/disk-evict-c").unwrap();
    let key_c = CacheKey::from_url(url_c.clone());
    let mut writer_c =
        HttpCacheStore::start_entry(&store, &key_c, build_stored_variant_meta(&url_c, 0))
            .await
            .expect("third entry should be created");
    writer_c
        .write(bytes::Bytes::from_static(b"cccc"))
        .expect("body write should succeed");
    writer_c.finish().expect("body finish should succeed");

    assert_eq!(HttpCacheStore::lookup(&store, &key_a).await.len(), 1);
    assert!(HttpCacheStore::lookup(&store, &key_b).await.is_empty());
    assert_eq!(HttpCacheStore::lookup(&store, &key_c).await.len(), 1);
}

/// A cache instance that has never seen a key must still serve it from the
/// backing store.
///
/// This is what a disk backend exists for: the process that stored the entry is
/// gone, and the one that starts next has an empty live map. Without it the
/// store is written and never read, every request misses, and each cold start
/// appends a duplicate variant — which is exactly what a device run showed
/// before this was fixed. Both backends are checked, because the memory store
/// hides the bug: in one process its live map is always warm.
async fn cache_serves_stored_entry_to_a_new_instance_impl<S, F>(make_store: F)
where
    S: HttpCacheStore + 'static,
    F: Fn() -> S,
{
    let url = ServoUrl::parse("https://servo.org/cache-across-instances").unwrap();
    let request = RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .pipeline_id(Some(TEST_PIPELINE_ID))
    .origin(url.origin())
    .build();

    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = Response::new(url.clone(), timing);
    response
        .headers
        .insert(EXPIRES, HeaderValue::from_str("-10").unwrap());
    response.body = servo_arc::Arc::new(Mutex::new(ResponseBody::Done(b"persisted".to_vec())));

    let writer = HttpCache::with_store(Box::new(make_store()));
    writer.store(&request, &response).await;
    drop(writer);

    // Built only now, the way the next process would build it.
    let second = make_store();

    // Split the two failure modes: nothing persisted, versus persisted but not
    // read back.
    let persisted = HttpCacheStore::lookup(&second, &CacheKey::new(&request)).await;
    assert_eq!(
        persisted.len(),
        1,
        "the store must hold exactly one variant after the first instance wrote it"
    );

    // A second cache over the same storage, standing in for the next process.
    let reader = HttpCache::with_store(Box::new(second));
    let mut done_chan = None;
    let cached = reader
        .construct_response(&request, &mut done_chan)
        .await
        .expect("a cache instance with an empty live map must serve from the store");
    assert!(
        matches!(*cached.body.lock(), ResponseBody::Done(ref body) if body == b"persisted"),
        "the served body must be the stored bytes"
    );
}

#[tokio::test]
async fn memory_store_serves_stored_entry_to_a_new_instance() {
    let store = MemoryStore::default();
    cache_serves_stored_entry_to_a_new_instance_impl(|| store.clone()).await;
}

#[tokio::test]
async fn disk_store_serves_stored_entry_to_a_new_instance() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    cache_serves_stored_entry_to_a_new_instance_impl(|| {
        DiskStore::new(temp_dir.path(), 8 * 1024 * 1024)
    })
    .await;
}

/// A URL whose request headers differ from fetch to fetch must not accumulate
/// one stored variant per fetch.
///
/// Variants are keyed by the fields the stored response's `Vary` names, so a
/// response without `Vary` has exactly one variant no matter what the request
/// headers looked like. A device run found a tracking beacon holding 46
/// variants of the same URL while every other URL held one, which is what this
/// pins.
async fn store_contract_replaces_variant_when_response_does_not_vary_impl<S: HttpCacheStore>(
    store: &S,
) {
    let url = ServoUrl::parse("https://servo.org/beacon").unwrap();
    let key = CacheKey::from_url(url.clone());

    for nth in 0..4 {
        let mut meta = build_stored_variant_meta(&url, 0);
        // Something that differs per fetch and that no `Vary` names.
        meta.set_final_url(url.clone());
        let mut request_headers = HeaderMap::new();
        request_headers.insert(
            http::header::HeaderName::from_static("x-request-id"),
            HeaderValue::from_str(&format!("{nth}")).unwrap(),
        );
        let meta = meta.with_request_headers_for_test(request_headers);
        let mut writer = HttpCacheStore::start_entry(store, &key, meta)
            .await
            .expect("entry should be created");
        writer
            .write(Bytes::from_static(b"beacon"))
            .expect("body write should succeed");
        writer.finish().expect("body finish should succeed");
    }

    assert_eq!(
        HttpCacheStore::lookup(store, &key).await.len(),
        1,
        "a response without Vary must keep exactly one stored variant"
    );
}

#[tokio::test]
async fn memory_store_contract_replaces_variant_when_response_does_not_vary() {
    let store = MemoryStore::default();
    store_contract_replaces_variant_when_response_does_not_vary_impl(&store).await;
}

#[tokio::test]
async fn disk_store_contract_replaces_variant_when_response_does_not_vary() {
    let temp_dir = tempdir().expect("disk cache temp dir should be created");
    let store = DiskStore::new(temp_dir.path(), 8 * 1024 * 1024);
    store_contract_replaces_variant_when_response_does_not_vary_impl(&store).await;
}
