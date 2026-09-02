// zohara-update -- Zohara OS updates center.
//
// TODO: full port. The Python original is 478 lines, four panels
// (Zohara/system/kernel/drivers), each with Check / Install / Cancel
// buttons + a log TextView. The Rust port in update/ is a stub for now;
// the welcome/ crate is fully ported and committed as the reference.
//
// Plan:
//   1. Add the libadwaita dependencies (already in workspace)
//   2. Port one panel at a time, starting with the System & Apps one
//      (simplest, just `pacman -Syu`)
//   3. Reuse the helper functions from the welcome crate's pattern
//   4. Wire each panel's check/install buttons to a tokio process
//      that streams stdout/stderr to a TextView via glib::MainLoop
//   5. Move the global palette and helper functions to common/ once
//      the welcome and update crates both use them
fn main() {
    eprintln!("zohara-update: not yet ported (TODO)");
    eprintln!("See src/main.rs in this crate for the migration plan.");
    std::process::exit(1);
}
