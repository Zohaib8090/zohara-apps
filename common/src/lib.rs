// Common types and helpers shared across the zohara-apps workspace.
//
// Crates in this workspace are small, focused tools. Anything they all
// need (a pacman wrapper, a D-Bus client, a logging init) lives here
// so we don't duplicate code.
//
// At the moment this crate is mostly empty. As migrations land, helpers
// move here from each binary's `main.rs`.

pub fn workspace_marker() -> &'static str {
    "zohara-common-ok"
}
