//! Place shard pager and CAS volume mapper.
//!
//! After a bounded header check, the production path maps the file and
//! decodes an owned [`PlaceSnap`]. This crate never builds a `Proposal`
//! and does not enable `klotho-world/mutate`.
//!
//! mmap is compiled out under Miri (`cfg(miri)`). Tests still cover decode
//! via [`place_snap_from_bytes`]:
//!
//! ```text
//! cargo +nightly miri test -p klotho-stream
//! ```

#![allow(unsafe_code)]
#![warn(missing_docs)]

mod catalog;
mod codec;
mod error;
mod map;
mod present;

pub use catalog::{StreamCatalog, blob, map_place};
pub use codec::{
    CATALOG_COMPILER, CATALOG_KIND, CATALOG_MAGIC, CATALOG_MAX_PLACES, CATALOG_MAX_VOLUMES,
    CATALOG_VERSION, CatalogDesc, KCAS_HEADER_LEN, KCAS_MAGIC, KCAS_VERSION, KPLC_MAGIC,
    KPLC_VERSION, KcasEntry, PLACE_HEADER_LEN, PlaceHeader, PlaceRef, VolumeRef, decode_catalog,
    decode_kcas, encode_catalog, encode_kcas, encode_place_shard, file_hash, parse_kcas_header,
    parse_place_header, place_snap_from_bytes,
};
pub use error::StreamError;
pub use klotho_prove::{CATALOG_CAP, KCAS_VOLUME_CAP, MAX_BLOB_BYTES, MAX_BLOBS, PLACE_SHARD_CAP};
pub use klotho_world::{MAX_PLACE_ROWS, PlaceRow, PlaceSnap};
pub use present::{ClusterLod, PresentResidency, TextureResident, plan_residency};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_match_documented_numbers() {
        assert_eq!(CATALOG_CAP, 64 * 1024 * 1024);
        assert_eq!(KCAS_VOLUME_CAP, 4usize * 1024 * 1024 * 1024);
        assert_eq!(PLACE_SHARD_CAP, 32 * 1024 * 1024);
        assert_eq!(MAX_BLOB_BYTES, 32 * 1024 * 1024);
        assert_eq!(MAX_BLOBS, 16_384);
        assert_eq!(CATALOG_VERSION, 2);
        assert_eq!(CATALOG_MAGIC, *b"KWRP");
        assert_eq!(KCAS_MAGIC, *b"KCAS");
        assert_eq!(KPLC_MAGIC, *b"KPLC");
        assert_eq!(PLACE_HEADER_LEN, 96);
        assert_eq!(KCAS_HEADER_LEN, 12);
    }
}
