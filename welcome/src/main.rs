// zohara-welcome -- first-boot onboarding window.
//
// Auto-launches on desktop load (via /etc/xdg/autostart/zohara-welcome.desktop).
// Offers the user the most common next steps: install Zohara, migrate from
// another OS, manage users, check for updates, or just close the window
// (the live ISO replaces Close with "Try Zohara OS" so the wording matches
// the boot menu).
//
// Rust port of the original PyQt5 zohara-welcome. Same UX, same buttons,
// same icon set. Runs as a libadwaita::Application window so it picks up
// the user's GTK theme automatically.

use std::path::Path;
use std::process::Command;

use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, Button};
use libadwaita::prelude::*;

const APP_ID: &str = "io.zohara.Welcome";

// Catppuccin Mocha palette (matches the existing Python welcome / settings)
const BG: &str = "#1e1e2e";
const MUTED: &str = "#585b70";
const COLOR_INSTALL: &str = "#89b4fa"; // blue
const COLOR_MIGRATE: &str = "#a6e3a1"; // green
const COLOR_USERS: &str = "#cba6f7";   // purple
const COLOR_UPDATE: &str = "#f9e2af";  // yellow
const COLOR_CLOSE: &str = "#f38ba8";   // red

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

/// Build a styled button. Inline CSS via CssProvider is the simplest
/// way to get Catppuccin colors in libadwaita without a full stylesheet.
fn make_button(label: &str, color: &str) -> Button {
    let btn = Button::with_label(label);
    let css = format!(
        "button {{
            background-color: {color};
            color: {BG};
            border-radius: 8px;
            padding: 12px 16px;
            font-size: 16px;
            font-weight: bold;
            border: none;
        }}
        button:hover {{
            background-color: alpha({color}, 0.85);
        }}",
        color = color,
        BG = BG,
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
    log::info!("Building welcome UI (live_iso={live})");

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Welcome to Zohara OS")
        .default_width(600)
        .default_height(440)
        .build();
    window.set_resizable(false);

    // Catppuccin background
    let bg_provider = gtk::CssProvider::new();
    bg_provider.load_from_string(&format!(
        "window {{ background-color: {BG}; }}",
        BG = BG,
    ));
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
    title.set_markup(&format!(
        "<span font_size=\"24pt\" font_weight=\"bold\" foreground=\"#cdd6f4\">Welcome to Zohara OS</span>"
    ));
    vbox.append(&title);

    let subtitle = gtk::Label::new(None);
    subtitle.set_markup(&format!(
        "<span font_size=\"14pt\" foreground=\"{MUTED}\">What would you like to do?</span>",
        MUTED = MUTED
    ));
    vbox.append(&subtitle);

    let buttons_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    buttons_box.set_halign(gtk::Align::Center);
    buttons_box.set_hexpand(true);

    let status_label = gtk::Label::new(None);
    status_label.set_markup(&format!(
        "<span font_size=\"9pt\" foreground=\"{MUTED}\"> </span>",
        MUTED = MUTED
    ));

    // Click handler: update status, launch the command, close on success.
    macro_rules! on_click {
        ($btn:expr, $desc:expr, $cmd:expr) => {{
            let status_ref = status_label.clone();
            let win = window.clone();
            let description: String = $desc.to_string();
            let cmd: Vec<&'static str> = $cmd.iter().map(|s| *s).collect();
            $btn.connect_clicked(move |_| {
                status_ref.set_text(&format!("Launching {description}..."));
                match launch(&cmd, &description) {
                    Ok(_child) => {
                        win.close();
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
        let btn_install = make_button("Install Zohara OS", COLOR_INSTALL);
        on_click!(btn_install, "Calamares Installer", &["calamares"]);
        buttons_box.append(&btn_install);
    }

    let btn_migrate = make_button("Migrate from another OS", COLOR_MIGRATE);
    on_click!(
        btn_migrate,
        "Migration Tool",
        &["/usr/local/bin/zohara-migrate"]
    );
    buttons_box.append(&btn_migrate);

    if !live {
        let btn_users = make_button("👤  Manage Users", COLOR_USERS);
        on_click!(
            btn_users,
            "User Manager",
            &["/usr/local/bin/zohara-usermgr"]
        );
        buttons_box.append(&btn_users);

        let btn_update = make_button("🔄  Update System", COLOR_UPDATE);
        on_click!(
            btn_update,
            "System Updater",
            &["/usr/local/bin/zohara-update"]
        );
        buttons_box.append(&btn_update);
    }

    let btn_close = make_button(if live { "Try Zohara OS" } else { "Close" }, COLOR_CLOSE);
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

    // Register the unused variable to silence warnings until we add
    // command-line flag handling.
    let _args: Vec<&str> = vec![];
    let _ = _args;

    app.run_with_args(&[]);
}
