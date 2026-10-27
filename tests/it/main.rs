//! The integration tests, as one binary. Every file under `tests/` used to be its own
//! crate, and each linked the whole server with its debug info: some fifty executables of
//! nearly 400 MB, written on every build. A file that needs a process to itself (a global
//! subscriber or recorder) stays out as its own binary; see `[[test]]` in `Cargo.toml`.

#[path = "../common/mod.rs"]
mod common;

mod affiliation;
mod alerts;
mod auth_identity;
mod background;
mod character_scopes;
mod discord;
mod entities;
mod esi_client;
mod esi_token_rejected;
mod http_access;
mod http_alerts;
mod http_auth;
mod http_background_image;
mod http_instance;
mod http_killmails;
mod http_preferences;
mod http_reference;
mod http_settings;
mod http_tokens;
mod http_transfer;
mod http_ws;
mod killmail_names;
mod killmail_stream;
mod map_creators;
mod maps_access;
mod maps_concurrency;
mod maps_connection;
mod maps_eve_scout;
mod maps_events;
mod maps_ghost;
mod maps_history_format;
mod maps_ignored;
mod maps_jumps;
mod maps_map;
mod maps_personal_watchlist;
mod maps_signatures;
mod maps_signatures_paste;
mod maps_solar_system;
mod maps_tracking;
mod maps_transfer;
mod maps_undo_round_trip;
mod maps_watchlist;
mod maps_wire;
mod oauth_flow;
mod openapi;
mod presence_docking;
mod presence_offline;
mod retention;
mod sentry_tunnel;
mod skyhooks;
mod sovereignty;
mod threat;

/// A file here that `main.rs` does not declare is never compiled, so its tests would
/// quietly not run.
#[test]
fn every_file_here_is_declared() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it");
    let main = include_str!("main.rs");
    for entry in std::fs::read_dir(dir).expect("read tests/it") {
        let path = entry.expect("read an entry").path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().is_some_and(|e| e == "rs") && stem != "main" {
            assert!(
                main.contains(&format!("\nmod {stem};")),
                "tests/it/{stem}.rs is not declared in tests/it/main.rs"
            );
        }
    }
}
