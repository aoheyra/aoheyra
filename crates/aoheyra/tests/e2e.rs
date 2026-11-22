//! APP-17 end-to-end scenarios on the pinned aoMesh SimWorld.
//!
//! These tests drive the sibling simulator rather than reimplementing radio,
//! routing or identity behaviour here. Every scenario has a fixed seed and a
//! fixed schedule, and every assertion is explicit: no scenario is skipped
//! and no threshold is tuned to hide a regression. Run them with
//! `cargo test --workspace --all-features`; CI already uses that command.

#![cfg(feature = "e2e")]

use ao_mesh::sim::{
    Contact, Metrics, Mobility, Scenario, Topology, Traffic, baseline_scenarios,
    directory_scenarios, flood_scenarios, run,
};
use std::time::Duration;

/// Start from aoMesh's published baseline so every field has a real default.
fn base_scenario() -> Scenario {
    baseline_scenarios()
        .into_iter()
        .next()
        .expect("the published baseline has a line scenario")
}

struct Plan {
    name: &'static str,
    seed: u64,
    nodes: usize,
    topology: Topology,
    mobility: Mobility,
    traffic: Traffic,
    messages: usize,
    settle: Duration,
    run_for: Duration,
}

fn scenario(plan: Plan) -> Scenario {
    let mut scenario = base_scenario();
    scenario.name = plan.name;
    scenario.seed = plan.seed;
    scenario.nodes = plan.nodes;
    scenario.topology = plan.topology;
    scenario.mobility = plan.mobility;
    scenario.traffic = plan.traffic;
    scenario.messages = plan.messages;
    scenario.settle = plan.settle;
    scenario.run = plan.run_for;
    scenario
}

fn assert_clean(got: &Metrics) {
    assert_eq!(got.send_failures, 0, "{} refused a send", got.scenario);
    assert_eq!(
        got.duplicate_deliveries, 0,
        "{} delivered a duplicate",
        got.scenario
    );
}

/// Section 9 scenario 1: a sealed Direct message reaches its destination and
/// the sender learns about it through an authenticated receipt.
///
/// Two nodes are enough: aoMesh's destination reflection was fixed in #92, so
/// no third node is needed to avoid an underflow.
#[test]
fn scenario_1_direct_chat_delivers_with_receipt() {
    let mut scenario = scenario(Plan {
        name: "e2e-direct-chat",
        seed: 1_300_001,
        nodes: 2,
        topology: Topology::Line,
        mobility: Mobility::Static,
        traffic: Traffic::Balanced,
        messages: 1,
        settle: Duration::from_secs(10),
        run_for: Duration::from_secs(30),
    });
    scenario.receipts = true;
    let got = run(scenario);

    assert_eq!(got.scenario, "e2e-direct-chat");
    assert_eq!(got.submitted, 1);
    assert_eq!(got.delivered_by_origin[0], 1);
    assert_eq!(got.delivered, 1);
    assert!(
        got.delivery_receipts >= 1,
        "the sender did not receive a receipt"
    );
    assert_clean(&got);
}

/// Section 9 scenario 2: A and C are out of range, and the sealed message
/// reaches C through B on a three-node line.
#[test]
fn scenario_2_relay_reaches_node_out_of_range() {
    let scenario = scenario(Plan {
        name: "e2e-relay",
        seed: 1_300_002,
        nodes: 3,
        topology: Topology::Line,
        mobility: Mobility::Static,
        traffic: Traffic::Balanced,
        messages: 1,
        settle: Duration::from_secs(10),
        run_for: Duration::from_secs(30),
    });
    let got = run(scenario);

    // `links_after_settle` counts one directed neighbour entry per node, so
    // A-B and B-C contribute four entries on a three-node line.
    assert_eq!(
        got.links_after_settle, 4,
        "the line must only have A-B and B-C"
    );
    assert_eq!(got.submitted, 1);
    assert_eq!(got.delivered_by_origin[0], 1);
    assert_eq!(got.delivered, 1);
    assert!(
        got.transmissions >= 2,
        "delivery across two hops needs a relay transmission"
    );
    assert_clean(&got);
}

/// Section 9 scenario 3: B carries A's message while B-C is down. C comes
/// back into range later, receives the message and its receipt purges the
/// stored copy. The contact times are run-relative; traffic is submitted in
/// the first four fifths of the run.
#[test]
fn scenario_3_store_and_forward_purges_after_receipt() {
    let contacts = vec![
        Contact {
            at_ms: 0,
            a: 1,
            b: 2,
            up: false,
        },
        Contact {
            at_ms: 40_000,
            a: 1,
            b: 2,
            up: true,
        },
    ];
    let mut scenario = scenario(Plan {
        name: "e2e-store-and-forward",
        seed: 1_300_003,
        nodes: 3,
        topology: Topology::Scripted(contacts),
        mobility: Mobility::Scripted,
        traffic: Traffic::Balanced,
        messages: 1,
        settle: Duration::from_secs(10),
        run_for: Duration::from_secs(60),
    });
    scenario.spray = true;
    scenario.receipts = true;
    let got = run(scenario);

    assert_eq!(got.submitted, 1);
    assert_eq!(
        got.delivered_by_origin[0], 1,
        "C never received the carried item"
    );
    assert_eq!(got.delivered, 1);
    assert!(
        got.delivery_receipts >= 1,
        "A never learned that C received the item"
    );
    assert!(
        got.purges >= 1,
        "the authenticated receipt did not purge the carried copy"
    );
    assert_eq!(got.remaining_delivered_copies, 0);
    assert_clean(&got);
}

/// Section 9 scenario 4: eight phones in one room settle onto four links
/// each, below aoNetwork's production default of five, and the step-1 cycle
/// keeps the graph connected. Sealed Direct traffic over those links must be
/// delivered without refused sends.
///
/// All eight submissions are exercised; the destination reflection fix in
/// aoMesh#92 removed the old `m = 3` underflow.
#[test]
fn scenario_4_degree_cap_keeps_eight_node_graph_connected() {
    const NODES: usize = 8;
    const LINKS_PER_NODE: usize = 4;
    let scenario = scenario(Plan {
        name: "e2e-degree-cap",
        seed: 1_300_004,
        nodes: NODES,
        topology: Topology::Circulant { steps: vec![1, 2] },
        mobility: Mobility::Static,
        traffic: Traffic::Balanced,
        messages: NODES,
        settle: Duration::from_secs(30),
        run_for: Duration::from_secs(60),
    });
    let got = run(scenario);

    assert_eq!(got.submitted, NODES as u64);
    assert_eq!(got.delivered, NODES as u64);
    assert_eq!(
        got.links_after_settle,
        NODES * LINKS_PER_NODE,
        "every phone must settle on four directed neighbour entries"
    );
    let cap = ao_network::NetworkConfig::default().max_links() as usize;
    assert!(
        LINKS_PER_NODE <= cap,
        "four links per phone exceed aoNetwork's default cap of {cap}"
    );
    assert_clean(&got);
}

/// Section 9 scenario 5: a flooding origin is throttled by relay budgets
/// while honest origins keep their delivery ratio relative to an identical
/// no-flood baseline. The published aoMesh flood schedule is reused verbatim.
#[test]
fn scenario_5_flood_control_preserves_honest_delivery() {
    let flood = flood_scenarios()
        .into_iter()
        .next()
        .expect("the published flood scenario is configured");
    let (flood_origin, weight) = match flood.traffic {
        Traffic::Flood { origin, weight } => (origin, weight),
        Traffic::Balanced
        | Traffic::Broadcast { .. }
        | Traffic::MixedBroadcast { .. }
        | Traffic::Presence
        | Traffic::Telemetry { .. }
        | Traffic::Channel { .. } => panic!("the published flood case must use Flood traffic"),
    };
    let mut baseline = flood.clone();
    baseline.name = "e2e-flood-baseline";
    baseline.traffic = Traffic::Balanced;
    baseline.messages = flood.messages / 2;

    let bounded = run(flood.clone());
    let reference = run(baseline.clone());

    let round = weight + 1;
    let flood_submitted = flood.messages.div_ceil(round) as u64;
    let honest_submitted = (flood.messages as u64 - flood_submitted) as f64;
    let honest_delivered = |metrics: &Metrics| -> u64 {
        metrics
            .delivered_by_origin
            .iter()
            .enumerate()
            .filter(|(origin, _)| *origin != flood_origin)
            .map(|(_, count)| *count)
            .sum()
    };
    let baseline_submitted = (0..baseline.messages)
        .filter(|m| m % baseline.nodes != flood_origin)
        .count() as f64;

    assert!(
        bounded.delivered_by_origin[flood_origin] * 2 < flood_submitted,
        "the flood origin delivered {} of {} submissions",
        bounded.delivered_by_origin[flood_origin],
        flood_submitted
    );
    assert!(
        honest_delivered(&bounded) as f64 / honest_submitted
            >= 0.9 * (honest_delivered(&reference) as f64 / baseline_submitted),
        "honest delivery regressed against the no-flood baseline"
    );
    // Both schedules are long and deliberately saturate send queues, so
    // refused sends are expected. The acceptance gate is delivery ratio:
    // the flooder must be throttled while honest origins stay near control.
}

/// Section 9 scenario 6: the directory entry a map would render is bounded
/// and expires on its own schedule. The app-side location payload and map
/// view are covered by their own services; this suite asserts the mesh
/// directory facts they consume without faking an application payload.
#[test]
fn scenario_6_location_directory_entry_expires_on_schedule() {
    let scenario = directory_scenarios()
        .into_iter()
        .next()
        .expect("the published directory scenario is configured");
    let plan = scenario
        .directory
        .as_ref()
        .expect("the directory scenario has a plan");
    let capacity = plan.config.capacity;
    let stale_after_ms = plan.config.stale_after_ms;
    let got = run(scenario);

    assert_eq!(got.scenario, "directory-line-6");
    assert!(
        got.directory_peak_entries >= 1,
        "no node ever learned a map-visible entry"
    );
    assert!(
        got.directory_peak_entries <= capacity,
        "the directory exceeded its capacity"
    );
    assert!(
        got.directory_expirations >= 1,
        "stale map-visible entries were not reclaimed"
    );
    assert_eq!(got.directory_live_entries, 0);
    assert!(
        got.directory_last_removal_ms >= stale_after_ms,
        "an entry was removed before its staleness deadline"
    );
    assert_clean(&got);
}
