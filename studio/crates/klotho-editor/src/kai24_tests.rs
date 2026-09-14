//! KAI-24: multiplayer/live-service P0 production boundary.

use std::fs;
use std::path::{Path, PathBuf};

use klotho_core::{Hash, Tick};
use klotho_ir::from_ron;
use klotho_live::{
    AntiCheatAction, ConfigApproval, ConfigAuthority, ConfigPrincipal, ConfigRole, Deployment,
    EvidenceLevel, IncidentRunbook, LiveConfig, LiveConfigSet, Matchmaker, ModerationCategory,
    ModerationQueue, ModerationReport, MultiplayerProfile, NetworkMatrix, PlayerTicket,
    PrivacyPolicy, PublicEvidence, PublicGate, RegionStatus, Rehearsal, ReplicatedEncounterPattern,
    ServiceSession, TelemetryAggregate, achieved_level, forbid_overclaim, sidecar_action,
};
use klotho_net::{MAX_DEDICATED_PLAYERS, SidecarFlag};
use klotho_prove::hash_bytes;
use klotho_runtime::RuntimeProfile;
use serde::de::DeserializeOwned;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/netlock-live")
}

fn read<T: DeserializeOwned>(name: &str) -> T {
    from_ron(&fs::read_to_string(fixture().join(name)).unwrap()).unwrap()
}

fn no_rust_below(path: &Path) -> bool {
    fs::read_dir(path).unwrap().all(|entry| {
        let path = entry.unwrap().path();
        if path.is_dir() {
            no_rust_below(&path)
        } else {
            path.extension().and_then(|extension| extension.to_str()) != Some("rs")
        }
    })
}

fn no_placeholders(path: &Path) {
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            no_placeholders(&entry.unwrap().path());
        }
        return;
    }
    if path.extension().and_then(|extension| extension.to_str()) == Some("md") {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let lower = text.to_ascii_lowercase();
    for needle in ["todo", "tbd", "fixme", "placeholder", "lorem ipsum", "xxxx"] {
        assert!(
            !lower.contains(needle),
            "{} contains {needle}",
            path.display()
        );
    }
}

fn profile() -> MultiplayerProfile {
    read("profile.ron")
}

fn config_approvals(config: &LiveConfig) -> Vec<ConfigApproval> {
    let hash = config.content_hash();
    [
        (ConfigRole::Design, "Economy Owner"),
        (ConfigRole::Privacy, "Privacy Owner"),
        (ConfigRole::Operations, "Operations Owner"),
    ]
    .into_iter()
    .map(|(role, name)| ConfigApproval {
        role,
        principal: ConfigPrincipal::Human(name.into()),
        config_hash: hash,
    })
    .collect()
}

#[test]
fn fixture_is_data_only_and_selects_the_shooter_regression_profile() {
    no_placeholders(&fixture());
    assert!(no_rust_below(&fixture()));
    let profile = profile();
    profile.validate().unwrap();
    assert_eq!(profile.auth_hz, RuntimeProfile::AaaShooter.auth_hz() as u16);
    assert_eq!(
        profile.rewind_ticks,
        RuntimeProfile::AaaShooter.budget().rewind_ticks
    );
    assert_eq!(usize::from(profile.max_players), MAX_DEDICATED_PLAYERS);
    assert_eq!(RuntimeProfile::AaaAdventure.auth_hz(), 30);

    let encounter: ReplicatedEncounterPattern = read("encounter.ron");
    encounter.validate(&profile).unwrap();
    assert_eq!(encounter.players, 32);
    assert!(
        encounter
            .journeys
            .iter()
            .any(|journey| journey == "reconnect")
    );
}

#[test]
fn matchmaking_reconnect_and_regional_failover_are_bounded() {
    let profile = profile();
    let mut matchmaker = Matchmaker::new(profile.clone()).unwrap();
    for (id, at, skill) in [(3, 2, 1010), (1, 0, 1000), (2, 1, 990)] {
        matchmaker
            .enqueue(PlayerTicket {
                id,
                account: hash_bytes(&id.to_le_bytes()),
                region: "eu-central".into(),
                platform: "desktop-crossplay".into(),
                skill,
                party_size: 1,
                enqueued_at: Tick(at),
            })
            .unwrap();
    }
    let statuses = [
        RegionStatus {
            region: "eu-central".into(),
            healthy: true,
            service_latency_ms: 20,
        },
        RegionStatus {
            region: "us-east".into(),
            healthy: true,
            service_latency_ms: 80,
        },
    ];
    let lobby = matchmaker.form_lobby(&statuses, 50).unwrap();
    assert_eq!(
        lobby
            .tickets
            .iter()
            .map(|ticket| ticket.id)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let mut session = ServiceSession::from_lobby(&lobby);
    let grant = session.disconnect(2, 100, &profile).unwrap();
    session.reconnect(&grant, 110).unwrap();
    let failover = [
        RegionStatus {
            region: "eu-central".into(),
            healthy: false,
            service_latency_ms: 0,
        },
        RegionStatus {
            region: "us-east".into(),
            healthy: true,
            service_latency_ms: 80,
        },
    ];
    assert_eq!(session.failover(&profile, &failover).unwrap(), "us-east");
}

#[test]
fn economy_and_experiments_are_signed_atomic_and_observational() {
    let config: LiveConfig = read("config.ron");
    let authority = ConfigAuthority::from_bytes([24; 32]);
    let signed = authority
        .sign(config.clone(), &config_approvals(&config))
        .unwrap();
    signed.verify().unwrap();
    assert_eq!(
        signed
            .assign("matchmaking-window", hash_bytes(b"account-7"))
            .unwrap(),
        signed
            .assign("matchmaking-window", hash_bytes(b"account-7"))
            .unwrap()
    );
    let policy: PrivacyPolicy = read("privacy.ron");
    policy.validate().unwrap();
    TelemetryAggregate {
        metric: "win_rate_permille".into(),
        cohort: 40,
        value: 512,
        config_hash: signed.hash,
    }
    .validate(policy.minimum_cohort)
    .unwrap();

    let mut next = config;
    next.economy.version = 2;
    next.economy.values.insert("win_bonus".into(), 40);
    let next = authority
        .sign(next.clone(), &config_approvals(&next))
        .unwrap();
    let original = signed.hash;
    let mut set = LiveConfigSet::new(signed).unwrap();
    set.install(next).unwrap();
    assert_ne!(set.current.hash, original);
    assert_eq!(set.rollback().unwrap(), original);

    let mut tampered = set.current.clone();
    tampered
        .config
        .economy
        .values
        .insert("win_bonus".into(), 9999);
    assert!(tampered.verify().is_err());
}

#[test]
fn moderation_privacy_and_sidecar_integration_have_no_world_authority() {
    assert_eq!(sidecar_action(SidecarFlag::None), AntiCheatAction::Allow);
    assert_eq!(
        sidecar_action(SidecarFlag::StaleFire),
        AntiCheatAction::RejectStaleIntent
    );
    assert_eq!(
        sidecar_action(SidecarFlag::CmdRate),
        AntiCheatAction::Disconnect
    );

    let policy: PrivacyPolicy = read("privacy.ron");
    let mut queue = ModerationQueue::default();
    queue
        .submit(
            ModerationReport {
                id: 1,
                reporter: hash_bytes(b"reporter"),
                subject: hash_bytes(b"subject"),
                category: ModerationCategory::Cheating,
                evidence_commitment: hash_bytes(b"protected-case"),
                retention_days: 14,
            },
            &policy,
        )
        .unwrap();
    assert_eq!(queue.pending(), 1);
    queue.resolve(1).unwrap();
    assert_eq!(queue.pending(), 0);

    let manifest = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../engine/crates/klotho-live/Cargo.toml"),
    )
    .unwrap();
    for forbidden in [
        "klotho-world",
        "klotho-commit",
        "klotho-trace",
        "klotho-runtime",
    ] {
        assert!(!manifest.contains(forbidden));
    }
}

#[test]
fn network_incident_deploy_and_claim_gates_stay_public_p0() {
    let matrix: NetworkMatrix = read("network.ron");
    let report = matrix.evaluate(&profile()).unwrap();
    assert_eq!(report.passed_cases, 4);
    let rehearsals: Vec<Rehearsal> = read("rehearsals.ron");
    let gate = PublicGate::evaluate(report, &rehearsals).unwrap();
    assert_eq!(gate.rehearsals, 1);

    let runbook: IncidentRunbook = read("runbook.ron");
    let previous = hash_bytes(b"live-v1");
    let next = hash_bytes(b"live-v2");
    let mut deployment = Deployment::start(next, previous, vec![1, 10, 50, 100], &runbook).unwrap();
    assert_eq!(deployment.advance(true).unwrap(), 10);
    assert!(deployment.advance(false).is_err());
    assert_eq!(deployment.rollback(), previous);

    let public: PublicEvidence = read("evidence/public.ron");
    public.validate().unwrap();
    let level = achieved_level(&public, None, false, None);
    assert_eq!(level, EvidenceLevel::P0);
    assert_eq!(
        level.public_statement(),
        "P0 public multiplayer/live boundary"
    );
    assert!(forbid_overclaim("multiplayer production-ready", level).is_err());
    forbid_overclaim(level.public_statement(), level).unwrap();
    assert_ne!(public.id(), Hash::ZERO);
}
