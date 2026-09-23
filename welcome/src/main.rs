// zohara-welcome -- first-boot onboarding window.
//
// Auto-launches on desktop load (via /etc/xdg/autostart/zohara-welcome.desktop).
// Offers the user the most common next steps: install Zohara, migrate from
// another OS, manage users, check for updates, or just close the window
// (the live ISO replaces Close with "Try Zohara OS" so the wording matches
// the boot menu).
//
// Rust port of the original PyQt5 zohara-welcome. Same UX, same buttons.
//
// UI language: flat fills, one OS-level accent color, no gradients, no
// emoji -- matches the redesign covered in zohara-settings' docs/UI-REDESIGN.md.
// Runs as a libadwaita::Application window.

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, Button};
use libadwaita::prelude::*;

const APP_ID: &str = "io.zohara.Welcome";

// Flat, single-accent palette -- matches zohara-settings' data/win11.css.
// FALLBACK_ACCENT is only used if ~/.config/zohara/theme.json can't be
// read (e.g. zohara-settings has never been opened yet); see accent_color().
const BG: &str = "#1c1c1e";
const MUTED: &str = "#8a8a90";
const SECONDARY_BG: &str = "rgba(255,255,255,0.07)";
const FALLBACK_ACCENT: &str = "#4c8dff";

/// Reads the accent color zohara-settings' Personalization page persists.
/// This is what "OS-level theming" means in practice: apps read the same
/// file rather than hardcoding their own palette. Falls back quietly if
/// the file doesn't exist yet or isn't valid JSON.
fn accent_color() -> String {
    let path = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            std::path::PathBuf::from(home).join(".config")
        })
        .join("zohara")
        .join("theme.json");

    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("accent").and_then(|a| a.as_str()).map(str::to_string))
        .unwrap_or_else(|| FALLBACK_ACCENT.to_string())
}

/// Returns true if we're running from the live ISO (vs. an installed system).
fn is_live_iso() -> bool {
    Path::new("/run/archiso").exists()
}

/// Returns true if the current effective UID is 0.
fn is_root() -> bool {
    // /proc/self/status contains Uid:\t<ruid>\t<euid>\t<suid>\t<fsuid>
    let Ok(s) = std::fs::read_to_string("/proc/self/status") else {
        return false;
    };
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            // euid is the second field.
            if let Some(euid) = rest.split_whitespace().nth(1) {
                return euid == "0";
            }
        }
    }
    false
}

/// Launch a subprocess. If we're not root, prepend `pkexec` so the user
/// gets a polkit prompt. Returns the spawned child (still running) or an
/// error message.
///
/// NOTE: `Ok` here only means the OS accepted the exec -- it does NOT mean
/// the program did anything useful. A stub binary that spawns fine and
/// immediately exit(1)s (zohara-migrate/zohara-usermgr today) still
/// returns `Ok`. Callers must not treat `Ok` alone as "it worked" -- see
/// `on_click!`'s handling below, which is the actual fix for that.
fn launch(cmd: &[&str], description: &str) -> Result<std::process::Child, String> {
    let full_cmd: Vec<&str> = if is_root() {
        cmd.to_vec()
    } else {
        let mut v = vec!["pkexec"];
        v.extend_from_slice(cmd);
        v
    };

    log::info!("Launching {description}: {}", full_cmd.join(" "));

    Command::new(full_cmd[0])
        .args(&full_cmd[1..])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            log::error!("Failed to launch {description}: {e}");
            format!("{e}")
        })
}

/// Build a flat, single-color button (no per-feature color, no gradient).
/// `primary: true` fills with the OS accent; otherwise it's a flat neutral
/// surface, matching zohara-settings' win11-primary-btn / win11-secondary-btn.
fn make_button(label: &str, accent: &str, primary: bool) -> Button {
    let btn = Button::with_label(label);
    let (bg, fg) = if primary {
        (accent.to_string(), BG.to_string())
    } else {
        (SECONDARY_BG.to_string(), "#ffffff".to_string())
    };
    let css = format!(
        "button {{
            background-color: {bg};
            color: {fg};
            border-radius: 6px;
            padding: 12px 16px;
            font-size: 14px;
            font-weight: 600;
            border: none;
        }}
        button:hover {{
            background-color: alpha({bg}, 0.85);
        }}"
    );
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    btn
}

fn build_ui(app: &Application) {
    let live = is_live_iso();
    let accent = accent_color();
    log::info!("Building welcome UI (live_iso={live}, accent={accent})");

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Welcome to Zohara OS")
        .default_width(600)
        .default_height(460)
        .build();
    window.set_resizable(false);

    let bg_provider = gtk::CssProvider::new();
    bg_provider.load_from_string(&format!("window {{ background-color: {BG}; }}"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &bg_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 20);
    vbox.set_margin_top(28);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(28);
    vbox.set_margin_end(28);
    vbox.set_halign(gtk::Align::Center);
    vbox.set_valign(gtk::Align::Center);
    window.set_child(Some(&vbox));

    let title = gtk::Label::new(None);
    title.set_markup(
        "<span font_size=\"22pt\" font_weight=\"bold\" foreground=\"#ffffff\">Welcome to Zohara OS</span>",
    );
    vbox.append(&title);

    let subtitle = gtk::Label::new(None);
    subtitle.set_markup(&format!(
        "<span font_size=\"12pt\" foreground=\"{MUTED}\">What would you like to do?</span>"
    ));
    vbox.append(&subtitle);

    let buttons_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
    buttons_box.set_halign(gtk::Align::Center);
    buttons_box.set_hexpand(true);

    let status_label = gtk::Label::new(None);
    status_label.set_markup(&format!("<span font_size=\"9pt\" foreground=\"{MUTED}\"> </span>"));

    // Click handler: launch the command, then decide whether to close the
    // window based on what actually happened, not just whether spawn()
    // succeeded.
    //
    // A successful spawn only tells us the OS accepted the exec. To tell
    // "a real, still-running program" (e.g. Calamares) from "a stub that
    // spawned fine and immediately exit(1)ed" (zohara-migrate/usermgr
    // today), give the child a short window to finish on its own: a
    // background thread waits on it and records the result; a timeout on
    // the GTK main loop checks that result once it fires. If the child had
    // already exited with failure by then, the window stays open and shows
    // why, using its captured stderr, instead of silently closing with
    // nothing having visibly happened.
    macro_rules! on_click {
        ($btn:expr, $desc:expr, $cmd:expr) => {{
            let status_ref = status_label.clone();
            let win = window.clone();
            let description: String = $desc.to_string();
            let cmd: Vec<&'static str> = $cmd.iter().map(|s| *s).collect();
            $btn.connect_clicked(move |_| {
                status_ref.set_text(&format!("Launching {description}..."));
                match launch(&cmd, &description) {
                    Ok(child) => {
                        let outcome: Arc<Mutex<Option<(bool, String)>>> = Arc::new(Mutex::new(None));
                        let outcome_writer = outcome.clone();
                        std::thread::spawn(move || {
                            if let Ok(output) = child.wait_with_output() {
                                let ok = output.status.success();
                                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                                *outcome_writer.lock().unwrap() = Some((ok, stderr));
                            }
                        });

                        let status_ref = status_ref.clone();
                        let win = win.clone();
                        let description = description.clone();
                        glib::timeout_add_local_once(Duration::from_millis(600), move || {
                            match outcome.lock().unwrap().take() {
                                Some((false, stderr)) => {
                                    status_ref.set_text(&format!("{description} isn't ready yet"));
                                    show_error_dialog(&win, &description, &stderr);
                                }
                                // Either it exited successfully already, or
                                // (the common case for a real app) it's
                                // still running -- both close as before.
                                Some((true, _)) | None => win.close(),
                            }
                        });
                    }
                    Err(e) => {
                        status_ref.set_text(&format!("Error: {description} failed to start"));
                        show_error_dialog(&win, &description, &e);
                    }
                }
            });
        }};
    }

    if live {
        let btn_install = make_button("Install Zohara OS", &accent, true);
        on_click!(btn_install, "Calamares Installer", &["calamares"]);
        buttons_box.append(&btn_install);
    }

    let btn_migrate = make_button("Migrate from another OS", &accent, !live);
    on_click!(
        btn_migrate,
        "Migration Tool",
        &["/usr/local/bin/zohara-migrate"]
    );
    buttons_box.append(&btn_migrate);

    if !live {
        let btn_users = make_button("Manage Users", &accent, false);
        on_click!(
            btn_users,
            "User Manager",
            &["/usr/local/bin/zohara-usermgr"]
        );
        buttons_box.append(&btn_users);

        let btn_update = make_button("Update System", &accent, false);
        on_click!(
            btn_update,
            "System Updater",
            &["/usr/local/bin/zohara-update"]
        );
        buttons_box.append(&btn_update);
    }

    let btn_close = make_button(if live { "Try Zohara OS" } else { "Close" }, &accent, false);
    let win = window.clone();
    btn_close.connect_clicked(move |_| win.close());
    buttons_box.append(&btn_close);

    vbox.append(&buttons_box);
    vbox.append(&status_label);

    window.present();
}

/// Show a small error dialog. In GTK4, MessageDialog doesn't have a
/// blocking .run() method; we use .present() and connect to the
/// "response" signal to close it.
fn show_error_dialog(parent: &ApplicationWindow, description: &str, details: &str) {
    let details = if details.is_empty() {
        "(no output on stderr)".to_string()
    } else {
        details.to_string()
    };
    let body = format!(
        "Could not launch {description}.\n\nDetails:\n{details}\n\nDebug log: /tmp/zohara-welcome.log"
    );
    let dialog = gtk::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .buttons(gtk::ButtonsType::Close)
        .message_type(gtk::MessageType::Error)
        .title("Launch Error")
        .text("Could not launch the requested application.")
        .secondary_text(&body)
        .build();
    dialog.connect_response(|d, _| d.close());
    dialog.present();
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .init();

    // Make sure /tmp/zohara-welcome.log exists for parity with the
    // Python version (which logs there).
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/zohara-welcome.log");

    log::info!("=== zohara-welcome starting (Rust port) ===");
    log::info!("Live ISO: {is_live}", is_live = is_live_iso());

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run_with_args::<&str>(&[]);
}
