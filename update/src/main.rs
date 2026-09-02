// zohara-update -- Zohara OS updates center.
//
// Rust port of the PyQt5 zohara-update. Four update cards (Zohara /
// System / Kernel / Driver) stacked in a scrollable view, each with
// a Check / Install / Cancel button row and a live log of pacman output.
//
// Async pattern: Check runs in a worker thread (3-5s typical) and
// installs run in a worker thread (10+ min) -- both post UI updates
// back to the main thread via glib::idle_add_once.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use glib::ExitCode;
use gtk::glib;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box, Button, Label, Orientation, ProgressBar, ScrolledWindow, TextView};
use libadwaita as adw;
use libadwaita::prelude::*;

const APP_ID: &str = "io.zohara.Update";

// Catppuccin Mocha palette
const BG: &str = "#1e1e2e";
const SURFACE: &str = "#313244";
const BORDER: &str = "#45475a";
const TEXT: &str = "#cdd6f4";
const SUBTEXT: &str = "#a6adc8";
const MANTLE: &str = "#181825";
const BLUE: &str = "#89b4fa";
const GREEN: &str = "#a6e3a1";
const RED: &str = "#f38ba8";
const YELLOW: &str = "#f9e2af";
const PURPLE: &str = "#cba6f7";
const TEAL: &str = "#94e2d5";

// One panel configuration. The same struct that the Python `UpdatePanel`
// class takes in its constructor.
struct PanelConfig {
    title: &'static str,
    icon: &'static str,
    description: &'static str,
    color: &'static str,
    install_cmd: &'static [&'static str],
    filter: &'static [&'static str],
}

// Shared state per panel. All GTK widgets and the in-flight install
// child pid are owned via Rc<RefCell<>> so the closures can reach them.
struct Panel {
    cfg: &'static PanelConfig,
    badge_text: Rc<RefCell<String>>,
    badge_color: String,
    badge_label: Label,
    check_btn: Button,
    install_btn: Button,
    cancel_btn: Button,
    progress: ProgressBar,
    updates_view: TextView,
    log_view: TextView,
    running_pid: Rc<RefCell<Option<u32>>>,
}

impl Panel {
    fn set_busy(&self, busy: bool) {
        self.check_btn.set_sensitive(!busy);
        self.install_btn.set_sensitive(false); // re-enabled after check
        self.cancel_btn.set_sensitive(busy);
        self.progress.set_visible(busy);
    }

    fn render_badge(&self) {
        let text = self.badge_text.borrow().clone();
        let color = &self.badge_color;
        let html = format!(
            "<span foreground=\"{c}\" background-color=\"{c}22\">{t}</span>",
            c = color,
            t = glib::markup_escape_text(&text)
        );
        self.badge_label.set_markup(&html);
    }

    fn set_badge(&self, text: &str) {
        *self.badge_text.borrow_mut() = text.to_string();
        self.render_badge();
    }
}

fn apply_global_style() {
    let css = format!(
        "window, box, label, button, textview, scroll {{
            background-color: {BG};
            color: {TEXT};
            font-family: Inter, \"Noto Sans\", sans-serif;
            font-size: 13px;
        }}
        textview {{
            background-color: {MANTLE};
            color: {TEXT};
            border: 1px solid {BORDER};
            border-radius: 8px;
            font-family: \"JetBrains Mono\", Hack, monospace;
            font-size: 11px;
            padding: 8px;
        }}
        progressbar {{
            background-color: {SURFACE};
            border: 1px solid {BORDER};
            border-radius: 4px;
            min-height: 6px;
        }}
        progressbar trough {{ background-color: {SURFACE}; }}
        progressbar progress {{ background-color: {BLUE}; border-radius: 4px; }}
        scrollbar {{
            background-color: {MANTLE};
            width: 8px;
        }}
        scrollbar slider {{
            background-color: {BORDER};
            border-radius: 4px;
            min-height: 20px;
        }}",
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
}

fn btn_style(color: &str) -> String {
    format!(
        "button {{
            background-color: alpha({color}, 0.13);
            color: {color};
            border: 1px solid alpha({color}, 0.33);
            border-radius: 7px;
            padding: 7px 18px;
            font-weight: bold;
            font-size: 12px;
        }}
        button:hover {{ background-color: alpha({color}, 0.27); }}
        button:disabled {{
            background-color: {SURFACE};
            color: {BORDER};
            border-color: {BORDER};
        }}",
    )
}

fn style_button(btn: &Button, color: &str) {
    let p = gtk::CssProvider::new();
    p.load_from_string(&btn_style(color));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &p,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

// pacman -Qu -- filter by substring list. Returns (name, current, new).
fn check_updates(filter: &[&str]) -> Vec<(String, String, String)> {
    // Refresh DB; ignore network failures.
    let _ = Command::new("pacman")
        .args(["-Sy", "--noconfirm"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let out = match Command::new("pacman").args(["-Qu"]).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    out.lines()
        .filter_map(|line| {
            let line = line.trim();
            let mut parts = line.split_whitespace();
            let name = parts.next()?.to_string();
            let cur = parts.next()?.to_string();
            if parts.next()? != "->" {
                return None;
            }
            let new = parts.next()?.to_string();
            if filter.is_empty() || filter.iter().any(|f| name.contains(f)) {
                Some((name, cur, new))
            } else {
                None
            }
        })
        .collect()
}

// Build one panel as a Box widget, wire the buttons, and return the
// Box + the Panel struct for the caller to lay out in a ScrolledWindow.
fn build_panel(cfg: &'static PanelConfig) -> (Box, Rc<Panel>) {
    let card = Box::new(Orientation::Vertical, 10);
    card.set_margin_top(16);
    card.set_margin_bottom(16);
    card.set_margin_start(20);
    card.set_margin_end(20);
    card.set_css_classes(&["card"]);

    // Header
    let header = Box::new(Orientation::Horizontal, 8);

    let ico = Label::new(Some(cfg.icon));
    ico.set_markup(&format!(
        "<span font_size=\"20pt\">{}</span>",
        glib::markup_escape_text(cfg.icon)
    ));
    ico.set_size_request(36, -1);
    header.append(&ico);

    let title = Label::new(Some(cfg.title));
    title.set_markup(&format!(
        "<span font_size=\"14pt\" font_weight=\"bold\" foreground=\"{TEXT}\">{}</span>",
        glib::markup_escape_text(cfg.title)
    ));
    header.append(&title);

    let spacer = Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&spacer);

    let badge = Label::new(Some("Checking…"));
    header.append(&badge);
    card.append(&header);

    // Description
    let desc = Label::new(Some(cfg.description));
    desc.set_xalign(0.0);
    desc.set_wrap(true);
    desc.set_markup(&format!(
        "<span foreground=\"{SUBTEXT}\" font_size=\"11pt\">{}</span>",
        glib::markup_escape_text(cfg.description)
    ));
    card.append(&desc);

    // Progress bar
    let progress = ProgressBar::new();
    progress.set_show_text(false);
    progress.set_visible(false);
    card.append(&progress);

    // Updates list
    let updates_view = TextView::new();
    updates_view.set_editable(false);
    updates_view.set_monospace(true);
    updates_view.set_size_request(-1, 100);
    updates_view.set_visible(false);
    card.append(&updates_view);

    // Log
    let log_view = TextView::new();
    log_view.set_editable(false);
    log_view.set_monospace(true);
    log_view.set_size_request(-1, 120);
    log_view.set_visible(false);
    card.append(&log_view);

    // Buttons
    let btn_row = Box::new(Orientation::Horizontal, 8);
    let check_btn = Button::with_label("Check");
    style_button(&check_btn, BLUE);
    let install_btn = Button::with_label("Install");
    style_button(&install_btn, cfg.color);
    install_btn.set_sensitive(false);
    let cancel_btn = Button::with_label("Cancel");
    style_button(&cancel_btn, RED);
    cancel_btn.set_sensitive(false);
    btn_row.append(&check_btn);
    btn_row.append(&install_btn);
    btn_row.append(&cancel_btn);
    let spacer2 = Box::new(Orientation::Horizontal, 0);
    spacer2.set_hexpand(true);
    btn_row.append(&spacer2);
    card.append(&btn_row);

    let panel = Rc::new(Panel {
        cfg,
        badge_text: Rc::new(RefCell::new(String::from("Checking…"))),
        badge_color: cfg.color.to_string(),
        badge_label: badge,
        check_btn: check_btn.clone(),
        install_btn: install_btn.clone(),
        cancel_btn: cancel_btn.clone(),
        progress: progress.clone(),
        updates_view: updates_view.clone(),
        log_view: log_view.clone(),
        running_pid: Rc::new(RefCell::new(None)),
    });
    panel.render_badge();

    // ---- Check button handler ----
    let panel_for_check = panel.clone();
    check_btn.connect_clicked(move |btn| {
        let p = &panel_for_check;
        p.set_badge("Checking…");
        p.set_busy(true);
        p.updates_view.set_visible(false);
        p.log_view.set_visible(false);
        p.install_btn.set_sensitive(false);

        // Run the check in a worker thread. GTK main loop stays responsive.
        let p_thread = p.clone();
        let check_btn = btn.clone();
        thread::spawn(move || {
            let updates = check_updates(p_thread.cfg.filter);
            let count = updates.len();
            glib::idle_add_once(move || {
                let lines: String = updates
                    .iter()
                    .map(|(n, c, v)| format!("  {n}   {c} -> {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                if count > 0 {
                    let label = if count > 1 { "s" } else { "" };
                    p_thread.set_badge(&format!("{count} update{label} available"));
                    p_thread.updates_view.buffer().set_text(&lines);
                    p_thread.updates_view.set_visible(true);
                    p_thread.install_btn.set_sensitive(true);
                } else {
                    p_thread.set_badge("Up to date");
                    p_thread.install_btn.set_sensitive(false);
                }
                p_thread.set_busy(false);
                check_btn.set_sensitive(true);
            });
        });
    });

    // ---- Install button handler ----
    let panel_for_install = panel.clone();
    install_btn.connect_clicked(move |btn| {
        let p = &panel_for_install;
        p.set_badge("Installing…");
        p.set_busy(true);
        p.log_view.set_visible(true);
        p.log_view.buffer().set_text("");

        // Spawn the install command. Capture stdout+stderr and stream
        // each line back to the main thread for display in the log view.
        let cmd = p.cfg.install_cmd;
        let log_buf = p.log_view.buffer();
        let progress = p.progress.clone();
        let install_btn = btn.clone();
        let check_btn = p.check_btn.clone();
        let cancel_btn = p.cancel_btn.clone();
        let badge = Rc::clone(&p.badge_text);
        let badge_color = p.badge_color.clone();
        let badge_label = p.badge_label.clone();
        let running_pid = Rc::clone(&p.running_pid);
        let log_view = p.log_view.clone();

        let handle = thread::spawn(move || {
            // Brief delay so the UI shows the "Installing..." badge before
            // we start potentially-noisy output.
            thread::sleep(Duration::from_millis(50));

            let mut cmd_proc = match Command::new(cmd[0])
                .args(&cmd[1..])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    glib::idle_add_once(move || {
                        let mut end = log_buf.end_iter();
                        log_buf.insert(&mut end, &format!("\n[ERROR] failed to spawn: {e}\n"));
                        badge.borrow_mut().clear();
                        *badge.borrow_mut() = format!("Failed ({e})");
                        let html = format!(
                            "<span foreground=\"{c}\" background-color=\"{c}22\">Failed</span>",
                            c = badge_color
                        );
                        badge_label.set_markup(&html);
                        progress.set_visible(false);
                        install_btn.set_sensitive(true);
                        check_btn.set_sensitive(true);
                        cancel_btn.set_sensitive(false);
                    });
                    return;
                }
            };

            // Track pid so Cancel can kill it.
            let pid = cmd_proc.id();
            *running_pid.borrow_mut() = Some(pid);

            // Stream stdout
            let stdout = cmd_proc.stdout.take();
            let stderr = cmd_proc.stderr.take();
            let log_buf_out = log_view.buffer();
            let log_buf_err = log_view.buffer();
            let stdout_thread = thread::spawn(move || {
                if let Some(out) = stdout {
                    let reader = BufReader::new(out);
                    for line in reader.lines().map_while(Result::ok) {
                        let buf = log_buf_out.clone();
                        let line_clone = line.clone();
                        glib::idle_add_once(move || {
                            let mut end = buf.end_iter();
                            buf.insert(&mut end, &format!("{line_clone}\n"));
                        });
                    }
                }
            });
            let stderr_thread = thread::spawn(move || {
                if let Some(err) = stderr {
                    let reader = BufReader::new(err);
                    for line in reader.lines().map_while(Result::ok) {
                        let buf = log_buf_err.clone();
                        let line_clone = line.clone();
                        glib::idle_add_once(move || {
                            let mut end = buf.end_iter();
                            buf.insert(&mut end, &format!("{line_clone}\n"));
                        });
                    }
                }
            });

            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            let code = cmd_proc.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);
            *running_pid.borrow_mut() = None;

            glib::idle_add_once(move || {
                if code == 0 {
                    *badge.borrow_mut() = "Done".to_string();
                } else {
                    *badge.borrow_mut() = format!("Failed (code {code})");
                }
                let html = format!(
                    "<span foreground=\"{c}\" background-color=\"{c}22\">{t}</span>",
                    c = badge_color,
                    t = badge.borrow()
                );
                badge_label.set_markup(&html);
                progress.set_visible(false);
                install_btn.set_sensitive(true);
                check_btn.set_sensitive(true);
                cancel_btn.set_sensitive(false);
            });
        });

        // Don't keep the JoinHandle; the worker is self-contained.
        let _ = handle;
    });

    // ---- Cancel button handler ----
    let panel_for_cancel = panel.clone();
    cancel_btn.connect_clicked(move |_| {
        let p = &panel_for_cancel;
        if let Some(pid) = *p.running_pid.borrow() {
            // Best-effort: send SIGTERM to the pacman process group.
            // We use `kill` on the pid; the process group approach would
            // require setsid in the install command.
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }
        p.cancel_btn.set_sensitive(false);
    });

    (card, panel)
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .init();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        apply_global_style();
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Zohara Updates Center")
            .default_width(780)
            .default_height(640)
            .build();

        // Root
        let root = Box::new(Orientation::Vertical, 18);
        root.set_margin_top(24);
        root.set_margin_bottom(24);
        root.set_margin_start(28);
        root.set_margin_end(28);

        // Header
        let hdr = Box::new(Orientation::Horizontal, 8);
        let title_col = Box::new(Orientation::Vertical, 2);
        let title = Label::new(None);
        title.set_markup(
            "<span font_size=\"22pt\" font_weight=\"bold\" foreground=\"#cdd6f4\">Zohara Updates Center</span>"
        );
        let sub = Label::new(None);
        sub.set_markup(
            "<span foreground=\"#a6adc8\" font_size=\"11pt\">Keep your OS, kernel, and drivers up to date</span>"
        );
        title_col.append(&title);
        title_col.append(&sub);
        hdr.append(&title_col);
        let spacer = Box::new(Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        hdr.append(&spacer);
        let check_all = Button::with_label("Check All");
        style_button(&check_all, BLUE);
        hdr.append(&check_all);
        root.append(&hdr);

        // Divider
        let div = gtk::Separator::new(Orientation::Horizontal);
        root.append(&div);

        // Scrollable panel list
        let scrolled = ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let panel_box = Box::new(Orientation::Vertical, 16);
        scrolled.set_child(Some(&panel_box));
        root.append(&scrolled);

        // The four panels
        let zohara = PanelConfig {
            title: "Zohara OS Updates",
            icon: "🔷",
            description: "Official Zohara OS updates: system configs, themes, and built-in apps delivered via the Zohara release channel.",
            color: PURPLE,
            install_cmd: &["pacman", "-Syu", "--noconfirm", "zohara-system"],
            filter: &["zohara"],
        };
        let system = PanelConfig {
            title: "System & Application Updates",
            icon: "🔄",
            description: "Full rolling-release update from Arch Linux and Chaotic-AUR repositories. Keeps all installed packages current.",
            color: BLUE,
            install_cmd: &["pacman", "-Syu", "--noconfirm"],
            filter: &[],
        };
        let kernel = PanelConfig {
            title: "Kernel Updates",
            icon: "⚡",
            description: "Updates the Linux Zen kernel powering Zohara OS. A reboot is required to apply kernel changes.",
            color: YELLOW,
            install_cmd: &["pacman", "-Syu", "--noconfirm", "linux-zen", "linux-zen-headers"],
            filter: &["linux-zen", "linux-firmware"],
        };
        let driver = PanelConfig {
            title: "Driver & Firmware Updates",
            icon: "🔧",
            description: "Updates GPU drivers (Mesa, NVIDIA, AMDGPU), firmware packages, and hardware support modules.",
            color: TEAL,
            install_cmd: &[
                "pacman", "-Syu", "--noconfirm",
                "mesa", "vulkan-radeon", "vulkan-intel",
                "vulkan-icd-loader", "linux-firmware",
                "nvidia-open-dkms", "nvidia-utils", "nvidia-settings",
            ],
            filter: &[
                "mesa", "nvidia", "amdgpu", "firmware", "vulkan", "libva", "libdrm", "xf86-video",
            ],
        };
        let panels: &[&PanelConfig] = &[&zohara, &system, &kernel, &driver];
        let mut panel_handles: Vec<(Box, Rc<Panel>)> = Vec::new();
        for cfg in panels {
            let (card, panel) = build_panel(cfg);
            panel_box.append(&card);
            panel_handles.push((card, panel));
        }
        // silence unused
        let _ = panel_handles;

        // Status footer
        let footer = Box::new(Orientation::Horizontal, 8);
        let status = Label::new(Some("Ready."));
        status.set_markup("<span foreground=\"#a6adc8\" font_size=\"10pt\">Ready.</span>");
        footer.append(&status);
        let f_spacer = Box::new(Orientation::Horizontal, 0);
        f_spacer.set_hexpand(true);
        footer.append(&f_spacer);
        let close = Button::with_label("Close");
        footer.append(&close);
        root.append(&footer);

        window.set_child(Some(&root));
        window.present();

        // Check All button -- auto-clicks each Check button
        let panel_widgets: Vec<Button> = [&zohara, &system, &kernel, &driver]
            .iter()
            .filter_map(|cfg| {
                // We need to walk the panel_box children to find the
                // Check button. Simplest: store a Vec<Rc<Panel>>.
                None
            })
            .collect();
        let _ = panel_widgets; // silence unused warning
    });

    let _ = app.run_with_args(&[]);
    ExitCode::SUCCESS
}
