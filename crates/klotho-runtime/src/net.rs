//! Optional listen-server helper. Default runtime play does not enable this.
//!
//! Feature `net` / `net-listen` expose [`listen_pair`]. Dedicated Role::Server
//! lives in `klotho-net` and the `klotho-server` bin (`net-dedicated`).

use klotho_core::Hash;
use klotho_net::{Client, Host, NetError, memory_session};

/// In-memory 2-player listen-server (host `PlayerId` 0, client `PlayerId` 1).
pub fn listen_pair(canon_hash: Hash) -> Result<(Host, Client), NetError> {
    memory_session(canon_hash)
}
