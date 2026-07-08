//! Baked-in Servo resources.
//!
//! Servo's `baked-in-resources` feature is a no-op on OpenHarmony (the `servo-default-resources`
//! crate is not compiled there), so the ArkWeb cdylib must supply the resource reader itself.
//! The bytes are embedded from the canonical `components/default-resources/resources/` directory
//! (single source of truth) and the reader is registered via `submit_resource_reader!`, which is
//! verified to work from a cdylib.

use std::path::PathBuf;

use servo::resources::{Resource, ResourceReaderMethods};

struct ArkWebResourceReader;

impl ResourceReaderMethods for ArkWebResourceReader {
    fn read(&self, file: Resource) -> Vec<u8> {
        let bytes: &[u8] = match file {
            Resource::BluetoothBlocklist => {
                include_bytes!("../../../components/default-resources/resources/gatt_blocklist.txt")
            },
            Resource::DomainList => {
                include_bytes!("../../../components/default-resources/resources/public_domains.txt")
            },
            Resource::HstsPreloadList => {
                include_bytes!(
                    "../../../components/default-resources/resources/hsts_preload.fstmap"
                )
            },
            Resource::BadCertHTML => {
                include_bytes!("../../../components/default-resources/resources/badcert.html")
            },
            Resource::NetErrorHTML => {
                include_bytes!("../../../components/default-resources/resources/neterror.html")
            },
            Resource::BrokenImageIcon => {
                include_bytes!("../../../components/default-resources/resources/rippy.png")
            },
            Resource::CrashHTML => {
                include_bytes!("../../../components/default-resources/resources/crash.html")
            },
            Resource::DirectoryListingHTML => {
                include_bytes!(
                    "../../../components/default-resources/resources/directory-listing.html"
                )
            },
            Resource::AboutMemoryHTML => {
                include_bytes!("../../../components/default-resources/resources/about-memory.html")
            },
            Resource::DebuggerJS => {
                include_bytes!("../../../components/default-resources/resources/debugger.js")
            },
            Resource::JsonViewerHTML => {
                include_bytes!("../../../components/default-resources/resources/json-viewer.html")
            },
        };
        bytes.to_owned()
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }
}

servo::submit_resource_reader!(&ArkWebResourceReader);
