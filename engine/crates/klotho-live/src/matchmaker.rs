//! Deterministic public matchmaking, lobby, session, and reconnect fixtures.

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::{Hash, Tick};
use klotho_prove::hash_bytes;
use serde::{Deserialize, Serialize};

use crate::{LiveError, MultiplayerProfile};

/// One pseudonymous player/party ticket.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerTicket {
    /// Stable ticket id.
    pub id: u64,
    /// Pseudonymous account hash; no raw account or IP.
    pub account: Hash,
    /// Preferred region.
    pub region: String,
    /// Platform pool.
    pub platform: String,
    /// Integer skill rating.
    pub skill: i32,
    /// Party size represented by this ticket.
    pub party_size: u16,
    /// Queue arrival tick.
    pub enqueued_at: Tick,
}

/// Ordered regional service state.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionStatus {
    /// Region id.
    pub region: String,
    /// Matchmaking/session service is healthy.
    pub healthy: bool,
    /// Observed service latency.
    pub service_latency_ms: u16,
}

/// Deterministically formed lobby.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Lobby {
    /// Content identity from sorted tickets and selected region.
    pub id: Hash,
    /// Hosting region.
    pub region: String,
    /// Platform pool.
    pub platform: String,
    /// Tickets in stable arrival/id order.
    pub tickets: Vec<PlayerTicket>,
    /// Total represented players.
    pub players: u16,
}

/// Reconnect capability for one ticket. The public fixture is deterministic
/// and is not a production credential implementation.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ReconnectGrant {
    /// Session identity.
    pub session: Hash,
    /// Ticket identity.
    pub ticket: u64,
    /// Expiry in service seconds.
    pub expires_at: u64,
    /// Integrity token bound to the other fields and public fixture secret.
    pub token: Hash,
}

/// Active service session. The authoritative simulation remains in the
/// dedicated Klotho server; this record cannot write it.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ServiceSession {
    /// Session identity.
    pub id: Hash,
    /// Current region.
    pub region: String,
    /// Lobby membership.
    pub members: BTreeSet<u64>,
    /// Disconnected tickets eligible for reconnect.
    pub reconnecting: BTreeMap<u64, ReconnectGrant>,
}

impl ServiceSession {
    /// Start from a formed lobby.
    #[must_use]
    pub fn from_lobby(lobby: &Lobby) -> Self {
        Self {
            id: lobby.id,
            region: lobby.region.clone(),
            members: lobby.tickets.iter().map(|ticket| ticket.id).collect(),
            reconnecting: BTreeMap::new(),
        }
    }

    /// Mark a member disconnected and issue a bounded public-fixture grant.
    pub fn disconnect(
        &mut self,
        ticket: u64,
        now_seconds: u64,
        profile: &MultiplayerProfile,
    ) -> Result<ReconnectGrant, LiveError> {
        if !self.members.remove(&ticket) {
            return Err(LiveError::Session("ticket is not an active member".into()));
        }
        let expires_at = now_seconds.saturating_add(u64::from(profile.reconnect_seconds));
        let grant = reconnect_grant(self.id, ticket, expires_at);
        self.reconnecting.insert(ticket, grant.clone());
        Ok(grant)
    }

    /// Rejoin with an exact, unexpired grant.
    pub fn reconnect(&mut self, grant: &ReconnectGrant, now_seconds: u64) -> Result<(), LiveError> {
        if grant.session != self.id
            || grant.expires_at < now_seconds
            || reconnect_grant(grant.session, grant.ticket, grant.expires_at).token != grant.token
            || self.reconnecting.get(&grant.ticket) != Some(grant)
        {
            return Err(LiveError::Session(
                "reconnect grant is invalid or expired".into(),
            ));
        }
        self.reconnecting.remove(&grant.ticket);
        self.members.insert(grant.ticket);
        Ok(())
    }

    /// Move service routing to the first healthy profile region. This does not
    /// migrate or mutate Projection; server handoff must use normal snapshots.
    pub fn failover(
        &mut self,
        profile: &MultiplayerProfile,
        statuses: &[RegionStatus],
    ) -> Result<String, LiveError> {
        let healthy: BTreeSet<&str> = statuses
            .iter()
            .filter(|status| status.healthy)
            .map(|status| status.region.as_str())
            .collect();
        let region = profile
            .regions
            .iter()
            .find(|region| healthy.contains(region.as_str()))
            .ok_or_else(|| LiveError::Session("no declared region is healthy".into()))?;
        self.region.clone_from(region);
        Ok(region.clone())
    }
}

fn reconnect_grant(session: Hash, ticket: u64, expires_at: u64) -> ReconnectGrant {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"klotho-public-reconnect-v1");
    bytes.extend_from_slice(session.as_bytes());
    bytes.extend_from_slice(&ticket.to_le_bytes());
    bytes.extend_from_slice(&expires_at.to_le_bytes());
    ReconnectGrant {
        session,
        ticket,
        expires_at,
        token: hash_bytes(&bytes),
    }
}

/// Stable public matchmaker. Completion order cannot affect lobby membership.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Matchmaker {
    profile: MultiplayerProfile,
    tickets: BTreeMap<u64, PlayerTicket>,
}

impl Matchmaker {
    /// Construct from an approved profile.
    pub fn new(profile: MultiplayerProfile) -> Result<Self, LiveError> {
        profile.validate()?;
        Ok(Self {
            profile,
            tickets: BTreeMap::new(),
        })
    }

    /// Queue a ticket after profile/PII checks.
    pub fn enqueue(&mut self, ticket: PlayerTicket) -> Result<(), LiveError> {
        if ticket.account == Hash::ZERO
            || ticket.party_size == 0
            || ticket.party_size > self.profile.max_players
            || !self.profile.regions.contains(&ticket.region)
            || !self.profile.platforms.contains(&ticket.platform)
            || self.tickets.contains_key(&ticket.id)
        {
            return Err(LiveError::Session("matchmaking ticket is invalid".into()));
        }
        self.tickets.insert(ticket.id, ticket);
        Ok(())
    }

    /// Form one lobby by `(arrival, id)` within the first healthy preferred
    /// region/platform pool. Skill spread is bounded by `max_skill_delta`.
    pub fn form_lobby(
        &mut self,
        statuses: &[RegionStatus],
        max_skill_delta: u32,
    ) -> Result<Lobby, LiveError> {
        let healthy: BTreeSet<&str> = statuses
            .iter()
            .filter(|status| status.healthy)
            .map(|status| status.region.as_str())
            .collect();
        let region = self
            .profile
            .regions
            .iter()
            .find(|region| healthy.contains(region.as_str()))
            .ok_or_else(|| LiveError::Session("no healthy matchmaking region".into()))?
            .clone();
        let mut candidates: Vec<_> = self
            .tickets
            .values()
            .filter(|ticket| ticket.region == region)
            .cloned()
            .collect();
        candidates.sort_by_key(|ticket| (ticket.enqueued_at, ticket.id));
        let first = candidates
            .first()
            .ok_or_else(|| LiveError::Session("matchmaking queue is empty".into()))?;
        let platform = first.platform.clone();
        let anchor_skill = first.skill;
        let mut selected = Vec::new();
        let mut players = 0u16;
        for ticket in candidates {
            if ticket.platform != platform
                || anchor_skill.abs_diff(ticket.skill) > max_skill_delta
                || players.saturating_add(ticket.party_size) > self.profile.max_players
            {
                continue;
            }
            players = players.saturating_add(ticket.party_size);
            selected.push(ticket);
        }
        if players < self.profile.min_players {
            return Err(LiveError::Session("not enough compatible players".into()));
        }
        for ticket in &selected {
            self.tickets.remove(&ticket.id);
        }
        let id = lobby_id(&region, &platform, &selected);
        Ok(Lobby {
            id,
            region,
            platform,
            tickets: selected,
            players,
        })
    }

    /// Number of queued tickets.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.tickets.len()
    }
}

fn lobby_id(region: &str, platform: &str, tickets: &[PlayerTicket]) -> Hash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(region.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(platform.as_bytes());
    for ticket in tickets {
        bytes.extend_from_slice(&ticket.id.to_le_bytes());
        bytes.extend_from_slice(ticket.account.as_bytes());
    }
    hash_bytes(&bytes)
}
