// zohara-usermgr -- TODO: port from PyQt5 version
//
// The Python original is ~437 lines, mostly a form-based UI for
// creating new user accounts. The Rust port will use libadwaita
// (same toolkit as welcome/update) with these fields:
//   - Username (text entry, validated against /etc/login.defs rules)
//   - Full name / GECOS
//   - UID (auto / explicit)
//   - Home directory
//   - Shell (dropdown: bash, zsh, fish)
//   - Groups (multi-select: wheel, audio, video, network, etc.)
//   - Password (twice, with strength meter)
//
// On submit, runs `useradd` (or `useradd -m` + `passwd`) as root via
// pkexec when not already root, and emits a D-Bus signal so the
// session manager can pick up the new account.
//
// This is a stub for now so the workspace builds; the real port
// is a follow-up.
fn main() {
    eprintln!("zohara-usermgr: not yet ported (TODO)");
    std::process::exit(1);
}
