/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![deny(missing_docs)]

//! File-backed HTTP cache store.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use futures_util::future::BoxFuture;
use futures_util::stream::{self, BoxStream, StreamExt};
use malloc_size_of::{MallocConditionalSizeOf, MallocSizeOf, MallocSizeOfOps};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::http_cache::CacheKey;
use crate::http_cache_store::{
    BodyHandle, BodyWriter, HttpCacheStore, StoreError, StoredVariant, StoredVariantMeta,
};

const INDEX_FILE_NAME: &str = "index.json";
const ENTRY_MAGIC: &[u8; 4] = b"HCD0";
const ENTRY_VERSION: u32 = 1;
const FOOTER_SIZE: usize = 4 + 4 + 8 + 8 + 32;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DiskIndexEntry {
    key_url: String,
    file_name: String,
    body_len: usize,
    last_used: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct DiskIndex {
    entries: HashMap<String, Vec<DiskIndexEntry>>,
    total_bytes: u64,
    next_seq: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DiskTrailer {
    key_url: String,
    meta: StoredVariantMeta,
}

#[derive(Clone, Debug)]
struct DiskFooter {
    trailer_len: u64,
    body_len: u64,
    checksum: [u8; 32],
}

/// File-backed HTTP cache store with rebuildable index state.
#[derive(Clone)]
pub struct DiskStore {
    root: Arc<PathBuf>,
    budget: u64,
    index: Arc<std::sync::Mutex<DiskIndex>>,
}

impl MallocConditionalSizeOf for DiskStore {
    fn conditional_size_of(&self, _ops: &mut MallocSizeOfOps) -> usize {
        self.index.lock().unwrap().total_bytes as usize + self.root.to_string_lossy().len()
    }
}

impl MallocSizeOf for DiskStore {
    fn size_of(&self, ops: &mut MallocSizeOfOps) -> usize {
        self.conditional_size_of(ops)
    }
}

impl DiskStore {
    /// Create or open a disk cache in `root` with a byte budget.
    pub fn new(root: impl AsRef<Path>, budget: u64) -> Self {
        let root = root.as_ref().to_path_buf();
        let _ = fs::create_dir_all(&root);
        let store = Self {
            root: Arc::new(root),
            budget,
            index: Arc::new(std::sync::Mutex::new(DiskIndex::default())),
        };
        store.rebuild_index();
        store
    }

    fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE_NAME)
    }

    fn entry_file_name(&self, key: &CacheKey, seq: u64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key.url().as_str().as_bytes());
        hasher.update(seq.to_le_bytes());
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write;
            let _ = write!(&mut hex, "{:02x}", byte);
        }
        format!("{hex}.entry")
    }

    fn write_index(&self) {
        let _ = fs::create_dir_all(&*self.root);
        if let Ok(index) = self.index.lock() {
            let tmp_path = self.index_path().with_extension("json.tmp");
            if let Ok(file) = File::create(&tmp_path) {
                let _ = serde_json::to_writer(file, &*index);
                let _ = fs::rename(tmp_path, self.index_path());
            }
        }
    }

    fn touch_entry_locked(index: &mut DiskIndex, file_name: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        for entries in index.entries.values_mut() {
            if let Some(entry) = entries
                .iter_mut()
                .find(|entry| entry.file_name == file_name)
            {
                entry.last_used = now;
                return;
            }
        }
    }

    fn evict_over_budget_locked(&self, index: &mut DiskIndex) {
        while index.total_bytes > self.budget {
            let mut victim: Option<(String, usize, DiskIndexEntry)> = None;
            for (key_url, entries) in &index.entries {
                for (position, entry) in entries.iter().enumerate() {
                    let replace = victim
                        .as_ref()
                        .map(|(_, _, current)| {
                            (entry.last_used, &entry.file_name) <
                                (current.last_used, &current.file_name)
                        })
                        .unwrap_or(true);
                    if replace {
                        victim = Some((key_url.clone(), position, entry.clone()));
                    }
                }
            }

            let Some((key_url, position, entry)) = victim else {
                break;
            };

            let _ = fs::remove_file(self.root.join(&entry.file_name));
            index.total_bytes = index.total_bytes.saturating_sub(entry.body_len as u64);
            let remove_key = if let Some(entries) = index.entries.get_mut(&key_url) {
                entries.remove(position);
                entries.is_empty()
            } else {
                false
            };
            if remove_key {
                index.entries.remove(&key_url);
            }
        }
    }

    fn touch_by_file_name(&self, file_name: &str) {
        if let Ok(mut index) = self.index.lock() {
            Self::touch_entry_locked(&mut index, file_name);
        }
        self.write_index();
    }

    fn rebuild_index(&self) {
        let index = self
            .read_index_from_disk()
            .unwrap_or_else(|| self.scan_directory());
        if let Ok(mut slot) = self.index.lock() {
            *slot = index;
        }
        self.write_index();
    }

    fn read_index_from_disk(&self) -> Option<DiskIndex> {
        let file = File::open(self.index_path()).ok()?;
        serde_json::from_reader(file).ok()
    }

    fn scan_directory(&self) -> DiskIndex {
        let mut index = DiskIndex::default();
        let Ok(entries) = fs::read_dir(&*self.root) else {
            return index;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|name| name.to_str()) == Some(INDEX_FILE_NAME) {
                continue;
            }
            if !path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext == "entry")
                .unwrap_or(false)
            {
                continue;
            }
            if let Some((key_url, meta, body_len)) = Self::read_entry(&path).ok().flatten() {
                let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
                let last_used = entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs())
                    .unwrap_or_else(|| {
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0)
                    });
                index.total_bytes = index.total_bytes.saturating_add(body_len as u64);
                index.next_seq = index.next_seq.saturating_add(1);
                index
                    .entries
                    .entry(key_url.clone())
                    .or_default()
                    .push(DiskIndexEntry {
                        key_url,
                        file_name,
                        body_len,
                        last_used,
                    });
                let _ = meta;
            }
        }
        index
    }

    fn checksum(body: &[u8], trailer: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(body);
        hasher.update(trailer);
        hasher.finalize().into()
    }

    fn footer_to_bytes(trailer_len: u64, body_len: u64, checksum: [u8; 32]) -> [u8; FOOTER_SIZE] {
        let mut bytes = [0_u8; FOOTER_SIZE];
        bytes[0..4].copy_from_slice(ENTRY_MAGIC);
        bytes[4..8].copy_from_slice(&ENTRY_VERSION.to_le_bytes());
        bytes[8..16].copy_from_slice(&trailer_len.to_le_bytes());
        bytes[16..24].copy_from_slice(&body_len.to_le_bytes());
        bytes[24..56].copy_from_slice(&checksum);
        bytes
    }

    fn footer_from_bytes(bytes: &[u8]) -> Option<DiskFooter> {
        if bytes.len() != FOOTER_SIZE || &bytes[0..4] != ENTRY_MAGIC {
            return None;
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
        if version != ENTRY_VERSION {
            return None;
        }
        Some(DiskFooter {
            trailer_len: u64::from_le_bytes(bytes[8..16].try_into().ok()?),
            body_len: u64::from_le_bytes(bytes[16..24].try_into().ok()?),
            checksum: bytes[24..56].try_into().ok()?,
        })
    }

    fn read_entry(path: &Path) -> Result<Option<(String, StoredVariantMeta, usize)>, ()> {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|_| ())?
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        if bytes.len() < FOOTER_SIZE {
            return Ok(None);
        }
        let footer = Self::footer_from_bytes(&bytes[bytes.len() - FOOTER_SIZE..]).ok_or(())?;
        let trailer_len = usize::try_from(footer.trailer_len).map_err(|_| ())?;
        let body_len = usize::try_from(footer.body_len).map_err(|_| ())?;
        if body_len + trailer_len + FOOTER_SIZE != bytes.len() {
            return Ok(None);
        }
        let trailer_start = body_len;
        let trailer_end = trailer_start + trailer_len;
        let trailer_bytes = &bytes[trailer_start..trailer_end];
        if Self::checksum(&bytes[..body_len], trailer_bytes) != footer.checksum {
            return Ok(None);
        }
        let trailer: DiskTrailer = serde_json::from_slice(trailer_bytes).map_err(|_| ())?;
        Ok(Some((trailer.key_url, trailer.meta, body_len)))
    }

    fn remove_entry_files(&self, key: &CacheKey) {
        let key_url = key.url().to_string();
        let entries = {
            let mut index = self.index.lock().unwrap();
            index.entries.remove(&key_url).unwrap_or_default()
        };
        for entry in entries {
            let _ = fs::remove_file(self.root.join(entry.file_name));
        }
        self.write_index();
    }

    fn clear_all(&self) {
        if let Ok(entries) = fs::read_dir(&*self.root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.file_name().and_then(|name| name.to_str()) == Some(INDEX_FILE_NAME) {
                    continue;
                }
                let _ = fs::remove_file(path);
            }
        }
        if let Ok(mut index) = self.index.lock() {
            *index = DiskIndex::default();
        }
        self.write_index();
    }

    fn body_path(&self, handle: &BodyHandle) -> Option<PathBuf> {
        handle.path().map(Path::to_path_buf)
    }

    fn bytes_for_body(path: &Path) -> Result<Vec<u8>, StoreError> {
        let bytes = fs::read(path).map_err(|_| StoreError::Closed)?;
        if bytes.len() < FOOTER_SIZE {
            return Err(StoreError::Closed);
        }
        let footer = Self::footer_from_bytes(&bytes[bytes.len() - FOOTER_SIZE..])
            .ok_or(StoreError::Closed)?;
        let body_len = usize::try_from(footer.body_len).map_err(|_| StoreError::Closed)?;
        if bytes.len() < body_len + FOOTER_SIZE {
            return Err(StoreError::Closed);
        }
        Ok(bytes[..body_len].to_vec())
    }
}

impl HttpCacheStore for DiskStore {
    fn lookup<'a>(&'a self, key: &'a CacheKey) -> BoxFuture<'a, Vec<StoredVariant>> {
        Box::pin(async move {
            let key_url = key.url().to_string();
            let mut touched = Vec::new();
            let metas = {
                let index = self
                    .index
                    .lock()
                    .expect("disk index mutex should not be poisoned");
                let Some(entries) = index.entries.get(&key_url) else {
                    return vec![];
                };
                entries
                    .iter()
                    .filter_map(|entry| {
                        let path = self.root.join(&entry.file_name);
                        Self::read_entry(&path).ok().flatten().map(|(_, meta, _)| {
                            touched.push(entry.file_name.clone());
                            StoredVariant {
                                meta,
                                body: BodyHandle::disk(path),
                            }
                        })
                    })
                    .collect::<Vec<_>>()
            };
            if let Ok(mut index) = self.index.lock() {
                for file_name in touched {
                    Self::touch_entry_locked(&mut index, &file_name);
                }
            }
            self.write_index();
            metas
        })
    }

    fn open_body<'a>(
        &'a self,
        handle: &'a BodyHandle,
    ) -> BoxFuture<'a, Result<BoxStream<'static, Bytes>, StoreError>> {
        Box::pin(async move {
            let Some(path) = self.body_path(handle) else {
                return Err(StoreError::Closed);
            };
            let bytes = Self::bytes_for_body(&path)?;
            if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
                self.touch_by_file_name(file_name);
            }
            Ok(stream::once(async move { Bytes::from(bytes) }).boxed())
        })
    }

    fn start_entry<'a>(
        &'a self,
        key: &'a CacheKey,
        meta: StoredVariantMeta,
    ) -> BoxFuture<'a, Result<Box<dyn BodyWriter>, StoreError>> {
        Box::pin(async move {
            let _ = fs::create_dir_all(&*self.root);
            let mut index = self
                .index
                .lock()
                .expect("disk index mutex should not be poisoned");
            let seq = index.next_seq;
            index.next_seq = index.next_seq.saturating_add(1);
            drop(index);

            let file_name = self.entry_file_name(key, seq);
            let path = self.root.join(&file_name);
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .map_err(|_| StoreError::Closed)?;
            Ok(Box::new(DiskBodyWriter {
                index: self.index.clone(),
                root: self.root.clone(),
                budget: self.budget,
                key_url: key.url().to_string(),
                file_name,
                path,
                file: Some(file),
                body_handle: BodyHandle::disk(self.root.join(self.entry_file_name(key, seq))),
                meta: StoredVariantMeta {
                    request_headers: crate::http_cache_store::sanitized_request_headers(
                        &meta.request_headers,
                    ),
                    ..meta
                },
                body_len: 0,
            }) as Box<dyn BodyWriter>)
        })
    }

    fn update_meta<'a>(
        &'a self,
        handle: &'a BodyHandle,
        meta: StoredVariantMeta,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let Some(path) = handle.path() else {
                return;
            };
            let Ok(bytes) = fs::read(path) else {
                return;
            };
            let Some(footer) =
                Self::footer_from_bytes(&bytes[bytes.len().saturating_sub(FOOTER_SIZE)..])
            else {
                return;
            };
            let body_len = footer.body_len as usize;
            let body = &bytes[..body_len];
            let Some((key_url, _, _)) = Self::read_entry(path).ok().flatten() else {
                return;
            };
            let trailer = DiskTrailer { key_url, meta };
            let Ok(trailer_bytes) = serde_json::to_vec(&trailer) else {
                return;
            };
            let checksum = Self::checksum(body, &trailer_bytes);
            let footer =
                Self::footer_to_bytes(trailer_bytes.len() as u64, body.len() as u64, checksum);
            let mut new_bytes = Vec::with_capacity(body.len() + trailer_bytes.len() + FOOTER_SIZE);
            new_bytes.extend_from_slice(body);
            new_bytes.extend_from_slice(&trailer_bytes);
            new_bytes.extend_from_slice(&footer);
            let _ = fs::write(path, new_bytes);
        })
    }

    fn remove<'a>(&'a self, key: &'a CacheKey) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.remove_entry_files(key) })
    }

    fn clear<'a>(&'a self) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.clear_all() })
    }

    fn entries<'a>(&'a self) -> BoxFuture<'a, Vec<net_traits::CacheEntryDescriptor>> {
        Box::pin(async move {
            let index = self
                .index
                .lock()
                .expect("disk index mutex should not be poisoned");
            index
                .entries
                .values()
                .flat_map(|entries| entries.iter())
                .filter(|entry| {
                    Self::read_entry(&self.root.join(&entry.file_name))
                        .ok()
                        .flatten()
                        .is_some()
                })
                .map(|entry| net_traits::CacheEntryDescriptor::new(entry.key_url.clone()))
                .collect()
        })
    }
}

struct DiskBodyWriter {
    index: Arc<std::sync::Mutex<DiskIndex>>,
    root: Arc<PathBuf>,
    budget: u64,
    key_url: String,
    file_name: String,
    path: PathBuf,
    file: Option<File>,
    body_handle: BodyHandle,
    meta: StoredVariantMeta,
    body_len: usize,
}

impl BodyWriter for DiskBodyWriter {
    fn body_handle(&self) -> BodyHandle {
        self.body_handle.clone()
    }

    fn write(&mut self, chunk: Bytes) -> Result<(), StoreError> {
        let Some(file) = self.file.as_mut() else {
            return Err(StoreError::Closed);
        };
        file.write_all(&chunk).map_err(|_| StoreError::Closed)?;
        self.body_len += chunk.len();
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), StoreError> {
        let file = self.file.take().ok_or(StoreError::Closed)?;
        file.sync_all().map_err(|_| StoreError::Closed)?;

        let trailer = DiskTrailer {
            key_url: self.key_url.clone(),
            meta: StoredVariantMeta {
                body_len: self.body_len,
                ..self.meta.clone()
            },
        };
        let trailer_bytes = serde_json::to_vec(&trailer).map_err(|_| StoreError::Closed)?;
        let checksum = DiskStore::checksum(
            &fs::read(&self.path).map_err(|_| StoreError::Closed)?,
            &trailer_bytes,
        );
        let footer =
            DiskStore::footer_to_bytes(trailer_bytes.len() as u64, self.body_len as u64, checksum);
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(|_| StoreError::Closed)?;
        file.write_all(&trailer_bytes)
            .map_err(|_| StoreError::Closed)?;
        file.write_all(&footer).map_err(|_| StoreError::Closed)?;
        file.sync_all().map_err(|_| StoreError::Closed)?;

        let store = DiskStore {
            root: self.root.clone(),
            budget: self.budget,
            index: self.index.clone(),
        };
        if let Ok(mut index) = self.index.lock() {
            index.total_bytes = index.total_bytes.saturating_add(self.body_len as u64);
            index
                .entries
                .entry(self.key_url.clone())
                .or_default()
                .push(DiskIndexEntry {
                    key_url: self.key_url.clone(),
                    file_name: self.file_name.clone(),
                    body_len: self.body_len,
                    last_used: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                });

            // Drop the variants this one supersedes. The index does not carry
            // the selecting headers, so each candidate's metadata is read back;
            // a key normally holds one variant, so this is a single small read.
            let fresh = StoredVariantMeta {
                body_len: self.body_len,
                ..self.meta.clone()
            };
            let mut superseded = Vec::new();
            if let Some(entries) = index.entries.get(&self.key_url) {
                for entry in entries {
                    if entry.file_name == self.file_name {
                        continue;
                    }
                    let path = self.root.join(&entry.file_name);
                    if let Ok(Some((_, meta, _))) = DiskStore::read_entry(&path) {
                        if crate::http_cache_store::supersedes(&fresh, &meta) {
                            superseded.push((entry.file_name.clone(), entry.body_len));
                        }
                    }
                }
            }
            if !superseded.is_empty() {
                if let Some(entries) = index.entries.get_mut(&self.key_url) {
                    entries.retain(|entry| {
                        !superseded.iter().any(|(name, _)| name == &entry.file_name)
                    });
                }
                for (file_name, body_len) in superseded {
                    let _ = fs::remove_file(self.root.join(&file_name));
                    index.total_bytes = index.total_bytes.saturating_sub(body_len as u64);
                }
            }

            store.evict_over_budget_locked(&mut index);
        }
        store.write_index();

        Ok(())
    }

    fn abort(mut self: Box<Self>) -> Result<(), StoreError> {
        let _ = self.file.take();
        let _ = fs::remove_file(&self.path);
        Ok(())
    }
}
