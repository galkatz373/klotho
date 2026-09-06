//! Headless runtime library: load a `.warp` under HLD §4 caps.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(all(feature = "aaa-adventure", feature = "aaa-shooter"))]
compile_error!("aaa-adventure and aaa-shooter are mutually exclusive runtime profiles");

mod epoch;
mod interest;
mod jobs;
mod profile;
mod residency;
mod stream;
mod warp;

pub use epoch::{HaltedEpoch, halt_for_epoch};
#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
pub use epoch::{HaltedServerEpoch, ServerEpochApplyError, halt_server_for_epoch};
pub use interest::apply_interest;
pub use jobs::ingest_island_jobs;
pub use klotho_stream::StreamCatalog;
pub use profile::RuntimeProfile;
pub use residency::residency_proposals;
pub use stream::{load_place_snap, open_stream_catalog};
pub use warp::{
    kernel_from_cooked, kernel_from_cooked_profile, load_cooked_warp, load_cooked_warp_capped,
    load_warp,
};

#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
mod net;
#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
pub use net::{ingest_server_intents, listen_pair};
