/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![deny(missing_docs)]

use std::path::Path;
#[cfg(feature = "disk-http-cache")]
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::future::BoxFuture;
use futures_util::stream::{self, BoxStream, StreamExt};
use http::HeaderMap;
use http::header::{IF_MODIFIED_SINCE, IF_NONE_MATCH};
use malloc_size_of::{MallocConditionalSizeOf, MallocSizeOf, MallocSizeOfOps};
use malloc_size_of_derive::MallocSizeOf;
use net_traits::http_status::HttpStatus;
use net_traits::{CacheEntryDescriptor, ResourceFetchTiming, ResourceTimingType};
use parking_lot::{Mutex as ParkingLotMutex, RwLock as ParkingLotRwLock};
use quick_cache::sync::Cache;
use serde::{Deserialize, Serialize};
use servo_config::pref;
use servo_url::ServoUrl;

use crate::http_cache::CacheKey;
use crate::http_cache_semantics::HttpCacheSemantics;

pub(crate) fn sanitized_request_headers(headers: &http::HeaderMap) -> http::HeaderMap {
    let mut headers = headers.clone();
    headers.remove(http::header::CACHE_CONTROL);
    headers.remove(http::header::PRAGMA);
    headers.remove(IF_MODIFIED_SINCE);
    headers.remove(IF_NONE_MATCH);
    headers.remove(http::header::IF_UNMODIFIED_SINCE);
    headers.remove(http::header::IF_MATCH);
    headers.remove(http::header::IF_RANGE);
    headers
}

/// The body handle used by the storage abstraction.
#[derive(Clone, Debug)]
pub struct BodyHandle(Arc<BodyHandleInner>);

impl PartialEq for BodyHandle {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

#[derive(Debug)]
enum BodyHandleInner {
    Memory(Arc<ParkingLotMutex<Vec<u8>>>),
    #[cfg(feature = "disk-http-cache")]
    Disk(PathBuf),
}

impl BodyHandle {
    /// Construct a new body handle.
    #[doc(hidden)]
    pub fn new(body: Vec<u8>) -> Self {
        Self(Arc::new(BodyHandleInner::Memory(Arc::new(
            ParkingLotMutex::new(body),
        ))))
    }

    /// Construct a disk-backed body handle.
    #[cfg(feature = "disk-http-cache")]
    pub(crate) fn disk(path: PathBuf) -> Self {
        Self(Arc::new(BodyHandleInner::Disk(path)))
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        match (&*self.0, &*other.0) {
            (BodyHandleInner::Memory(left), BodyHandleInner::Memory(right)) => {
                Arc::ptr_eq(left, right)
            },
            #[cfg(feature = "disk-http-cache")]
            (BodyHandleInner::Disk(left), BodyHandleInner::Disk(right)) => left == right,
            #[cfg(feature = "disk-http-cache")]
            _ => false,
        }
    }

    pub(crate) fn memory(&self) -> Option<&ParkingLotMutex<Vec<u8>>> {
        match &*self.0 {
            BodyHandleInner::Memory(body) => Some(body.as_ref()),
            #[cfg(feature = "disk-http-cache")]
            BodyHandleInner::Disk(_) => None,
        }
    }

    /// Return the on-disk file path, if this handle is disk-backed.
    pub fn path(&self) -> Option<&Path> {
        match &*self.0 {
            BodyHandleInner::Memory(_) => None,
            #[cfg(feature = "disk-http-cache")]
            BodyHandleInner::Disk(path) => Some(path.as_path()),
        }
    }
}

impl MallocConditionalSizeOf for BodyHandle {
    fn conditional_size_of(&self, ops: &mut MallocSizeOfOps) -> usize {
        match &*self.0 {
            BodyHandleInner::Memory(body) => body.conditional_size_of(ops),
            #[cfg(feature = "disk-http-cache")]
            BodyHandleInner::Disk(path) => path.to_string_lossy().len(),
        }
    }
}

impl MallocSizeOf for BodyHandle {
    fn size_of(&self, ops: &mut MallocSizeOfOps) -> usize {
        self.conditional_size_of(ops)
    }
}

/// Serializable cache policy summary stored with each variant.
#[derive(Clone, Debug, Deserialize, MallocSizeOf, PartialEq, Serialize)]
pub struct StoredCachePolicy {
    /// Whether the cached response is storable.
    pub(crate) cacheable: bool,
    /// Freshness lifetime derived from the cache policy.
    pub(crate) freshness_lifetime: std::time::Duration,
}

impl StoredCachePolicy {
    fn from_semantics(cache_semantics: &HttpCacheSemantics) -> Self {
        Self {
            cacheable: cache_semantics.is_cacheable(),
            freshness_lifetime: cache_semantics.freshness_lifetime(),
        }
    }
}

/// Whether a freshly stored variant supersedes an existing one.
///
/// RFC 9111 keys secondary variants on the header fields named by the stored
/// response's `Vary`. With no `Vary` there is only one variant per key, so a
/// newly stored response replaces what was there; with one, only the variant
/// whose nominated fields match is replaced. Without this a URL whose request
/// headers differ from fetch to fetch — a tracking beacon, say — accumulates
/// one variant per load until the byte budget evicts them. A device run showed
/// exactly that: one such URL held 46 variants while every other URL held one.
pub(crate) fn supersedes(fresh: &StoredVariantMeta, existing: &StoredVariantMeta) -> bool {
    let mut names = Vec::new();
    for value in fresh.response_headers.get_all(http::header::VARY) {
        let Ok(value) = value.to_str() else {
            return false;
        };
        for name in value.split(',') {
            let name = name.trim();
            if name == "*" {
                return false;
            }
            if !name.is_empty() {
                names.push(name.to_ascii_lowercase());
            }
        }
    }
    names.iter().all(|name| {
        fresh.request_headers.get(name.as_str()) == existing.request_headers.get(name.as_str())
    })
}

/// A stored variant as the semantics layer receives it: the metadata plus a
/// handle with which the backend can be asked for the body.
///
/// Metadata alone is not enough to serve a request. A process that starts with
/// a populated store — the reason a disk backend exists — has to be able to
/// reach the bytes belonging to a looked-up variant, and the handle is that
/// link. It dereferences to the metadata so callers that only need the
/// metadata read unchanged.
#[derive(Clone, Debug, MallocSizeOf, PartialEq)]
pub struct StoredVariant {
    /// Metadata describing the variant.
    pub meta: StoredVariantMeta,
    /// Handle the backend resolves to the variant's body.
    pub body: BodyHandle,
}

impl std::ops::Deref for StoredVariant {
    type Target = StoredVariantMeta;

    fn deref(&self) -> &Self::Target {
        &self.meta
    }
}

/// The metadata stored for a single cached variant.
#[derive(Clone, Debug, Deserialize, MallocSizeOf, PartialEq, Serialize)]
pub struct StoredVariantMeta {
    /// Request headers used for Vary matching.
    #[serde(
        deserialize_with = "hyper_serde::deserialize",
        serialize_with = "hyper_serde::serialize"
    )]
    pub(crate) request_headers: HeaderMap,
    /// Response headers to reconstruct the cached response.
    #[serde(
        deserialize_with = "hyper_serde::deserialize",
        serialize_with = "hyper_serde::serialize"
    )]
    pub(crate) response_headers: HeaderMap,
    /// Final URL associated with the cached response.
    pub(crate) final_url: ServoUrl,
    /// MIME type, if known.
    pub(crate) content_type: Option<String>,
    /// Character set, if known.
    pub(crate) charset: Option<String>,
    /// Cached HTTP status.
    pub(crate) status: HttpStatus,
    /// Serializable cache-policy snapshot.
    pub(crate) cache_policy: StoredCachePolicy,
    /// Redirect target recorded on the response.
    pub(crate) location_url: Option<Result<ServoUrl, String>>,
    /// Body length used for weighting and persistence.
    pub(crate) body_len: usize,
    /// URL chain for the cached response.
    pub(crate) url_list: Vec<ServoUrl>,
    /// Freshness lifetime.
    pub(crate) expires: std::time::Duration,
    /// stale-while-revalidate window.
    pub(crate) stale_while_revalidate: std::time::Duration,
}

impl StoredVariantMeta {
    /// Return the cached body length.
    pub fn body_len(&self) -> usize {
        self.body_len
    }

    /// Return the cached response headers.
    #[cfg(feature = "test-util")]
    pub fn response_headers(&self) -> &HeaderMap {
        &self.response_headers
    }

    /// Return the response final URL.
    pub fn final_url(&self) -> ServoUrl {
        self.final_url.clone()
    }

    /// Return the cached HTTP status.
    pub fn status(&self) -> HttpStatus {
        self.status.clone()
    }

    /// Update the final URL.
    pub fn set_final_url(&mut self, final_url: ServoUrl) {
        self.final_url = final_url;
    }

    /// Replace the request headers used for Vary matching.
    #[cfg(feature = "test-util")]
    pub fn with_request_headers_for_test(mut self, headers: HeaderMap) -> Self {
        self.request_headers = headers;
        self
    }

    /// Update the cached HTTP status.
    pub fn set_status(&mut self, status: HttpStatus) {
        self.status = status;
    }
}

/// Errors produced by the storage layer.
#[derive(Debug)]
pub enum StoreError {
    /// The body has already been finalized.
    Closed,
}

/// A writer that accepts body bytes for a cached entry.
pub trait BodyWriter: Send {
    /// Return the body handle owned by this writer.
    fn body_handle(&self) -> BodyHandle;

    /// Append bytes to the stored body.
    fn write(&mut self, chunk: Bytes) -> Result<(), StoreError>;

    /// Finish writing the body.
    fn finish(self: Box<Self>) -> Result<(), StoreError>;

    /// Abandon the body without finalizing it.
    fn abort(self: Box<Self>) -> Result<(), StoreError>;
}

/// The cache storage abstraction.
pub trait HttpCacheStore: Send + Sync + MallocSizeOf {
    /// Look up the stored variants for `key`, each with a handle to its body.
    fn lookup<'a>(&'a self, key: &'a CacheKey) -> BoxFuture<'a, Vec<StoredVariant>>;

    /// Open the body for `handle`.
    fn open_body<'a>(
        &'a self,
        handle: &'a BodyHandle,
    ) -> BoxFuture<'a, Result<BoxStream<'static, Bytes>, StoreError>>;

    /// Start a new entry for `key`.
    fn start_entry<'a>(
        &'a self,
        key: &'a CacheKey,
        meta: StoredVariantMeta,
    ) -> BoxFuture<'a, Result<Box<dyn BodyWriter>, StoreError>>;

    /// Update the metadata for an existing body.
    fn update_meta<'a>(
        &'a self,
        handle: &'a BodyHandle,
        meta: StoredVariantMeta,
    ) -> BoxFuture<'a, ()>;

    /// Remove cached resources associated with `key`.
    fn remove<'a>(&'a self, key: &'a CacheKey) -> BoxFuture<'a, ()>;

    /// Clear the store.
    fn clear<'a>(&'a self) -> BoxFuture<'a, ()>;

    /// Return the stored entry descriptors.
    fn entries<'a>(&'a self) -> BoxFuture<'a, Vec<CacheEntryDescriptor>>;
}

type CacheEntry = std::sync::Arc<ParkingLotRwLock<Vec<StoredVariant>>>;
type QuickCache = Cache<CacheKey, CacheEntry>;

/// A simple memory cache.
/// Elements will be evicted based on the cache heuristic.
#[derive(Clone)]
pub struct MemoryStore {
    entries: std::sync::Arc<QuickCache>,
}

impl MallocSizeOf for MemoryStore {
    fn size_of(&self, ops: &mut MallocSizeOfOps) -> usize {
        self.entries
            .iter()
            .map(|(_key, entry)| entry.try_read().map(|lock| lock.size_of(ops)).unwrap_or(0))
            .sum()
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        let size = pref!(network_http_cache_size)
            .try_into()
            .expect("http_cache_size needs to fit into u64");
        Self {
            entries: std::sync::Arc::new(Cache::new(size)),
        }
    }
}

impl MemoryStore {
    #[cfg(feature = "test-util")]
    /// Construct a store holding a fixed number of entries, for tests.
    pub fn with_capacity(entries: usize) -> Self {
        Self {
            entries: std::sync::Arc::new(Cache::new(entries)),
        }
    }
}

impl MemoryStore {
    /// Return the stored entry for `key`.
    pub(crate) async fn lookup(&self, key: &CacheKey) -> Vec<StoredVariant> {
        let Some(entry) = self.entries.get(key) else {
            return vec![];
        };
        let resources = entry.read();
        resources.clone()
    }

    /// Return descriptors for all entries.
    fn cache_entry_descriptors(&self) -> Vec<CacheEntryDescriptor> {
        self.entries
            .iter()
            .map(|(key, _)| CacheEntryDescriptor::new(key.url().to_string()))
            .collect()
    }

    /// Clear the contents of the cache.
    pub(crate) fn clear(&self) {
        self.entries.clear();
    }

    /// Open the body for `handle`.
    pub(crate) async fn open_body(
        &self,
        handle: &BodyHandle,
    ) -> Result<BoxStream<'static, Bytes>, StoreError> {
        if let Some(body) = handle.memory() {
            let body = body.lock().clone();
            return Ok(stream::once(async move { Bytes::from(body) }).boxed());
        }

        // A handle this store did not create belongs to another backend, whose
        // on-disk layout it does not know: reading the file here would hand back
        // the trailer and footer along with the body. Refuse it, and let the
        // caller fall back to a miss.
        Err(StoreError::Closed)
    }

    /// Insert a stored variant and return a body writer for it.
    pub(crate) async fn start_entry(
        &self,
        key: &CacheKey,
        meta: StoredVariantMeta,
    ) -> Result<Box<dyn BodyWriter>, StoreError> {
        let body = BodyHandle::new(vec![]);
        let stored_entry = StoredVariant {
            meta: StoredVariantMeta {
                request_headers: sanitized_request_headers(&meta.request_headers),
                ..meta
            },
            body: body.clone(),
        };
        let live_entry = match self.entries.get(key) {
            Some(existing) => {
                let _ = self.entries.remove(key);
                {
                    let mut resources = existing.write();
                    resources.push(stored_entry.clone());
                }
                let _ = self.entries.insert(key.clone(), existing.clone());
                existing
            },
            None => {
                let entry = std::sync::Arc::new(ParkingLotRwLock::new(vec![stored_entry.clone()]));
                match self.entries.get_value_or_guard_async(key).await {
                    Ok(existing) => {
                        {
                            let mut resources = existing.write();
                            resources.push(stored_entry.clone());
                        }
                        existing
                    },
                    Err(guard) => {
                        let _ = guard.insert(entry.clone());
                        entry
                    },
                }
            },
        };
        Ok(Box::new(MemoryBodyWriter {
            key: key.clone(),
            entries: self.entries.clone(),
            entry: live_entry,
            body,
        }))
    }

    /// Update the metadata for an existing body.
    pub(crate) async fn update_meta(&self, handle: &BodyHandle, meta: StoredVariantMeta) {
        if handle.path().is_some() {
            return;
        }
        for (key, entry) in self.entries.iter() {
            let needs_update = entry
                .read()
                .iter()
                .any(|resource| resource.body.ptr_eq(handle));
            if needs_update {
                reweight_entry(&self.entries, &key, &entry, || {
                    let mut resources = entry.write();
                    if let Some(resource) = resources
                        .iter_mut()
                        .find(|resource| resource.body.ptr_eq(handle))
                    {
                        resource.meta = meta.clone();
                    }
                });
                return;
            }
        }
    }

    /// Remove the cached resources associated with `key`.
    pub(crate) async fn remove(&self, key: &CacheKey) {
        if self.entries.get(key).is_some() {
            let _ = self.entries.remove(key);
        }
    }
}

impl HttpCacheStore for MemoryStore {
    fn lookup<'a>(&'a self, key: &'a CacheKey) -> BoxFuture<'a, Vec<StoredVariant>> {
        Box::pin(async move { self.lookup(key).await })
    }

    fn open_body<'a>(
        &'a self,
        handle: &'a BodyHandle,
    ) -> BoxFuture<'a, Result<BoxStream<'static, Bytes>, StoreError>> {
        Box::pin(async move { self.open_body(handle).await })
    }

    fn start_entry<'a>(
        &'a self,
        key: &'a CacheKey,
        meta: StoredVariantMeta,
    ) -> BoxFuture<'a, Result<Box<dyn BodyWriter>, StoreError>> {
        Box::pin(async move { self.start_entry(key, meta).await })
    }

    fn update_meta<'a>(
        &'a self,
        handle: &'a BodyHandle,
        meta: StoredVariantMeta,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.update_meta(handle, meta).await })
    }

    fn remove<'a>(&'a self, key: &'a CacheKey) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.remove(key).await })
    }

    fn clear<'a>(&'a self) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.clear() })
    }

    fn entries<'a>(&'a self) -> BoxFuture<'a, Vec<CacheEntryDescriptor>> {
        Box::pin(async move { self.cache_entry_descriptors() })
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::*;
    use crate::test::build_stored_variant_meta;

    #[tokio::test]
    async fn memory_store_open_body_streams_completed_bytes() {
        let store = MemoryStore::default();
        let url = ServoUrl::parse("https://servo.org/open-body").unwrap();
        let key = CacheKey::from_url(url.clone());

        let mut writer = store
            .start_entry(&key, build_stored_variant_meta(&url, 0))
            .await
            .expect("entry should be created");
        writer
            .write(Bytes::from_static(b"hello"))
            .expect("body write should succeed");
        writer.finish().expect("body finish should succeed");

        let body = store.entries.get(&key).expect("entry should exist").read()[0]
            .body
            .clone();
        let mut stream = store
            .open_body(&body)
            .await
            .expect("open_body should succeed");
        let bytes = stream
            .next()
            .await
            .expect("body stream should yield one chunk");
        assert_eq!(bytes, Bytes::from_static(b"hello"));
    }

    #[tokio::test]
    async fn memory_store_update_meta_updates_persisted_values() {
        let store = MemoryStore::default();
        let url = ServoUrl::parse("https://servo.org/update-meta").unwrap();
        let key = CacheKey::from_url(url.clone());

        let writer = store
            .start_entry(&key, build_stored_variant_meta(&url, 0))
            .await
            .expect("entry should be created");
        writer.finish().expect("body finish should succeed");

        let body = store.entries.get(&key).expect("entry should exist").read()[0]
            .body
            .clone();

        let mut updated = build_stored_variant_meta(&url, 0);
        let updated_url = ServoUrl::parse("https://servo.org/update-meta/updated").unwrap();
        updated.set_final_url(updated_url.clone());
        updated.set_status(StatusCode::ACCEPTED.into());
        store.update_meta(&body, updated.clone()).await;

        let stored = store.lookup(&key).await;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].final_url(), updated_url);
        assert_eq!(stored[0].status(), StatusCode::ACCEPTED);
    }
}

struct MemoryBodyWriter {
    key: CacheKey,
    entries: std::sync::Arc<QuickCache>,
    entry: CacheEntry,
    body: BodyHandle,
}

fn reweight_entry<F>(entries: &QuickCache, key: &CacheKey, entry: &CacheEntry, update: F)
where
    F: FnOnce(),
{
    if entries.get(key).is_some() {
        let _ = entries.remove(key);
        update();
        let _ = entries.insert(key.clone(), entry.clone());
    }
}

impl BodyWriter for MemoryBodyWriter {
    fn body_handle(&self) -> BodyHandle {
        self.body.clone()
    }

    fn write(&mut self, chunk: Bytes) -> Result<(), StoreError> {
        let Some(body) = self.body.memory() else {
            return Err(StoreError::Closed);
        };
        let mut body = body.try_lock().ok_or(StoreError::Closed)?;
        body.extend_from_slice(&chunk);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<(), StoreError> {
        let Self {
            key,
            entries,
            entry,
            body,
        } = *self;
        let Some(memory) = body.memory() else {
            return Err(StoreError::Closed);
        };
        let body_len = memory.lock().len();
        reweight_entry(&entries, &key, &entry, || {
            let mut resources = entry.write();
            let Some(index) = resources
                .iter()
                .position(|resource| resource.body.ptr_eq(&body))
            else {
                return;
            };
            resources[index].meta.body_len = body_len;
            let fresh = resources[index].meta.clone();
            resources.retain(|resource| {
                resource.body.ptr_eq(&body) || !supersedes(&fresh, &resource.meta)
            });
        });
        Ok(())
    }

    fn abort(self: Box<Self>) -> Result<(), StoreError> {
        let Self {
            key,
            entries,
            entry,
            body,
        } = *self;
        let Some(memory) = body.memory() else {
            return Err(StoreError::Closed);
        };
        memory.lock().clear();
        reweight_entry(&entries, &key, &entry, || {
            let mut resources = entry.write();
            if let Some(resource) = resources
                .iter_mut()
                .find(|resource| resource.body.ptr_eq(&body))
            {
                resource.meta.body_len = 0;
            }
        });
        Ok(())
    }
}

/// Build persisted metadata for tests.
pub fn build_stored_variant_meta(url: &ServoUrl, body_len: usize) -> StoredVariantMeta {
    let timing = ResourceFetchTiming::new(ResourceTimingType::Navigation);
    let mut response = net_traits::response::Response::new(url.clone(), timing);
    response
        .headers
        .insert(http::header::EXPIRES, http::HeaderValue::from_static("-10"));
    let cache_semantics = HttpCacheSemantics::new(&response);

    StoredVariantMeta {
        request_headers: http::HeaderMap::new(),
        response_headers: response.headers.clone(),
        final_url: url.clone(),
        content_type: Some("text/plain".into()),
        charset: Some("utf-8".into()),
        status: http::StatusCode::OK.into(),
        cache_policy: StoredCachePolicy::from_semantics(&cache_semantics),
        location_url: None,
        body_len,
        url_list: vec![url.clone()],
        expires: std::time::Duration::from_secs(60),
        stale_while_revalidate: std::time::Duration::ZERO,
    }
}

/// Collect the bytes from a stored body.
pub async fn collect_body<S: HttpCacheStore + ?Sized>(store: &S, handle: &BodyHandle) -> Bytes {
    let mut stream = HttpCacheStore::open_body(store, handle)
        .await
        .expect("body stream should open");
    stream
        .next()
        .await
        .expect("body stream should yield one chunk")
}
