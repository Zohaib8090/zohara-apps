// zohara-migrate -- TODO: port from PyQt5 version
//
// The Python original is ~546 lines. It's a multi-page wizard that
// imports packages, configs, and user data from another installed
// distro (Debian, Ubuntu, Fedora, Arch, etc.) into a fresh Zohara
// install. Pages:
//
//   1. Source detection: list detected installs under /mnt/* and
//      let the user pick one.
//   2. Package selection: cross-reference the source's installed
//      packages against what's available in Zohara's repos.
//   3. Config import: copy /home/*/.[a-z]* (dotfiles) and
//      /etc/<service>/* with safety filtering.
//   4. Account mapping: detect user accounts in the source, ask
//      whether to recreate them on the new system.
//   5. Summary + execute.
//
// The Rust port will reuse the multi-step pattern from the welcome
// crate (a single libadwaita::Application with stacked pages) and
// run rsync / pacman / useradd in worker threads (same pattern as
// zohara-update's install button).
//
// This is a stub for now so the workspace builds.
fn main() {
    eprintln!("zohara-migrate: not yet ported (TODO)");
    std::process::exit(1);
}
