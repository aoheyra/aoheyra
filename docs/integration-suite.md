# APP-17 integration suite

The suite lives in `crates/aoheyra/tests/e2e.rs` and runs on aoNetwork's
paused `SimWorld` through `ao_mesh::sim::run`. It is enabled by the `e2e`
Cargo feature; the normal CI command already enables it:

```bash
cargo test --workspace --all-features
```

Every scenario has a fixed seed and schedule. The assertions are exact or
directional on real simulator metrics; no scenario is ignored, skipped or
relaxed to make a run pass.

| Section 9 scenario | Test | Evidence |
|---|---|---|
| 1 Direct chat | `scenario_1_direct_chat_delivers_with_receipt` | one sealed Direct submission between two phones, one delivery, at least one authenticated receipt |
| 2 Relay | `scenario_2_relay_reaches_node_out_of_range` | three-node line has only A-B and B-C, delivery needs a relay transmission |
| 3 Store-and-forward | `scenario_3_store_and_forward_purges_after_receipt` | B-C is down at submission, returns later; delivery, receipt and purge all observed; no delivered copy remains |
| 4 Degree cap | `scenario_4_degree_cap_keeps_eight_node_graph_connected` | eight nodes settle on four links each, below aoNetwork's five-link default; the step-1 cycle keeps the graph connected; all eight submissions delivered |
| 5 Flood control | `scenario_5_flood_control_preserves_honest_delivery` | published aoMesh flood schedule; flooded origin's delivered count is less than half its submissions; honest ratio stays within 90% of the no-flood control |
| 6 Location | `scenario_6_location_directory_entry_expires_on_schedule` | node directory is populated, never exceeds capacity, and every stale entry expires on schedule |

All six scenarios use the current simulator schedules. The destination
reflection fix in aoMesh#92 removed the old two-node and eight-message
underflows, so no reduced schedule remains. Section 9 scenarios 7 (persistence
across a restart) and 8 (eight-hour soak) remain device evidence for AND-22
and AND-23; they are not simulated here.

The choice to use the sibling simulator rather than a bespoke harness is
recorded in `docs/adr/0016-end-to-end-suite.md`.
