//! Headless runtime library: load a `.warp` under HLD §4 caps.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod interest;
mod jobs;
mod residency;
mod stream;
mod warp;

pub use interest::apply_interest;
pub use jobs::ingest_island_jobs;
pub use klotho_stream::StreamCatalog;
pub use residency::residency_proposals;
pub use stream::{load_place_snap, open_stream_catalog};
pub use warp::{kernel_from_cooked, load_cooked_warp, load_cooked_warp_capped, load_warp};

#[cfg(feature = "net")]
mod net;
#[cfg(feature = "net")]
pub use net::listen_pair;
