//! Headless runtime library: load a `.warp` under HLD §4 caps.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod warp;

pub use warp::{kernel_from_cooked, load_cooked_warp, load_cooked_warp_capped, load_warp};
