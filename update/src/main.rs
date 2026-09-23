// zohara-update -- Zohara OS updates center.
//
// Rust port of the PyQt5 zohara-update. Four update cards (Zohara /
// System / Kernel / Driver) stacked in a scrollable view, each with
// a Check / Install / Cancel button row and a live log of pacman output.
//
// Threading model:
//   - UI state lives on the main thread (GTK widgets).
//   - Pacman invocations run as local async tasks (glib::spawn_future_local)
//     on a global Tokio runtime (3-5s for `pacman -Qu`, 10+ min for
//     `pacman -Syu`); see the TOKIO_RUNTIME comment below for why this
//     replaced an earlier std::thread::spawn design that didn't compile.
//   - Shared state (the running pid, read by the Cancel button) is
//     Arc<Mutex<Option<u32>>> so it's visible outside the async task.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use glib::ExitCode;
use gtk::glib;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box, Button, Label, Orientation, ProgressBar, ScrolledWindow, TextView};
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::io::AsyncBufReadExt;
use tokio::runtime::Runtime;

const APP_ID: &str = "io.zohara.Update";

// Genuinely `static` (not a local inside connect_activate's closure), so
// `&PANELS[i]` is `&'static PanelConfig` -- build_panel() requires that,
// since it hands the reference into Panel, which lives inside an Arc
// captured by long-lived GTK signal-handler closures. Four locals with
// the same values, taken by reference from inside a non-'static closure,
// is exactly the E0597 "does not live long enough" this used to hit.
static PANELS: [PanelConfig; 4] = [
    PanelConfig {
        title: "Zohara OS Updates",
        icon: "🔷",
        description: "Official Zohara OS updates: system configs, themes, and built-in apps delivered via the Zohara release channel.",
        color: PURPLE,
        install_cmd: &["pacman", "-Syu", "--noconfirm", "zohara-system"],
        filter: &["zohara"],
    },
    PanelConfig {
        title: "System & Application Updates",
        icon: "🔄",
        description: "Full rolling-release update from Arch Linux and Chaotic-AUR repositories. Keeps all installed packages current.",
        color: BLUE,
        install_cmd: &["pacman", "-Syu", "--noconfirm"],
        filter: &[],
    },
    PanelConfig {
        title: "Kernel Updates",
        icon: "⚡",
        description: "Updates the Linux Zen kernel powering Zohara OS. A reboot is required to apply kernel changes.",
        color: YELLOW,
        install_cmd: &["pacman", "-Syu", "--noconfirm", "linux-zen", "linux-zen-headers"],
        filter: &["linux-zen", "linux-firmware"],
    },
    PanelConfig {
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
    },
];

// A global multi-threaded Tokio runtime, entered once in main() and never
// exited. GTK widgets (Button, Label, TextView, ...) wrap a raw *mut
// c_void and are not Send, so std::thread::spawn can never move them --
// every earlier version of this file that tried (directly, or indirectly
// by moving an Arc<Panel> containing them) failed to compile with E0277
// "*mut c_void cannot be sent between threads safely". Background work
// below instead runs as a LOCAL future via glib::spawn_future_local
// (stays pinned to the main thread's GLib context, so it CAN capture
// widgets), and only the actual blocking work -- check_updates(), the
// pacman child process -- crosses onto this runtime, which needs to
// already be running for tokio::process / tokio::task::spawn_blocking to
// work. Same pattern as zohara-settings' src/main.rs.
static TOKIO_RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn tokio_runtime() -> &'static Runtime {
    TOKIO_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to initialize Tokio runtime")
    })
}

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

struct PanelConfig {
    title: &'static str,
    icon: &'static str,
    description: &'static str,
    color: &'static str,
    install_cmd: &'static [&'static str],
    filter: &'static [&'static str],
}

// Per-panel state. Widgets stay on the main thread; the only piece of
// state that moves into a worker thread is the running-pid, which
// is wrapped in Arc<Mutex<>>.
struct Panel {
    cfg: &'static PanelConfig,
    badge_text: Arc<Mutex<String>>,
    badge_color: String,
    badge_label: Label,
    check_btn: Button,
    install_btn: Button,
    cancel_btn: Button,
    progress: ProgressBar,
    updates_view: TextView,
    log_view: TextView,
    running_pid: Arc<Mutex<Option<u32>>>,
}

impl Panel {
    fn render_badge(&self) {
        let text = self.badge_text.lock().unwrap().clone();
        let color = &self.badge_color;
        let html = format!(
            "<span foreground=\"{c}\" background-color=\"{c}22\">{t}</span>",
            c = color,
            t = glib::markup_escape_text(&text)
        );
        self.badge_label.set_markup(&html);
    }

    fn set_badge(&self, text: &str) {
        *self.badge_text.lock().unwrap() = text.to_string();
        self.render_badge();
    }

    fn set_busy(&self, busy: bool) {
        self.check_btn.set_sensitive(!busy);
        self.install_btn.set_sensitive(false);
        self.cancel_btn.set_sensitive(busy);
        self.progress.set_visible(busy);
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

fn check_updates(filter: &[&str]) -> Vec<(String, String, String)> {
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

fn build_panel(cfg: &'static PanelConfig) -> (Box, Arc<Panel>) {
    let card = Box::new(Orientation::Vertical, 10);
    card.set_margin_top(16);
    card.set_margin_bottom(16);
    card.set_margin_start(20);
    card.set_margin_end(20);
    card.set_css_classes(&["card"]);

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

    let desc = Label::new(Some(cfg.description));
    desc.set_xalign(0.0);
    desc.set_wrap(true);
    desc.set_markup(&format!(
        "<span foreground=\"{SUBTEXT}\" font_size=\"11pt\">{}</span>",
        glib::markup_escape_text(cfg.description)
    ));
    card.append(&desc);

    let progress = ProgressBar::new();
    progress.set_show_text(false);
    progress.set_visible(false);
    card.append(&progress);

    let updates_view = TextView::new();
    updates_view.set_editable(false);
    updates_view.set_monospace(true);
    updates_view.set_size_request(-1, 100);
    updates_view.set_visible(false);
    card.append(&updates_view);

    let log_view = TextView::new();
    log_view.set_editable(false);
    log_view.set_monospace(true);
    log_view.set_size_request(-1, 120);
    log_view.set_visible(false);
    card.append(&log_view);

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

    let panel = Arc::new(Panel {
        cfg,
        badge_text: Arc::new(Mutex::new(String::from("Checking…"))),
        badge_color: cfg.color.to_string(),
        badge_label: badge,
        check_btn: check_btn.clone(),
        install_btn: install_btn.clone(),
        cancel_btn: cancel_btn.clone(),
        progress: progress.clone(),
        updates_view: updates_view.clone(),
        log_view: log_view.clone(),
        running_pid: Arc::new(Mutex::new(None)),
    });
    panel.render_badge();

    // The Check and Install handlers need to clone Arc<Panel> into a
    // worker thread. We give each handler a clone of the panel +
    // a clone of the GTK widget it needs to update. (GTK widgets are
    // not Send, so they cannot be moved into the worker; we use
    // glib::idle_add_once from the worker to push work back to the
    // main thread, where the widget handle lives.)
    let panel_for_check = panel.clone();
    let updates_view_for_check = updates_view.clone();
    let check_btn_for_check = check_btn.clone();
    let install_btn_for_check = install_btn.clone();
    // cfg.filter is &'static [&'static str] -- Copy, Send, needs no
    // cloning to cross into the blocking task below.
    let filter = cfg.filter;
    check_btn.connect_clicked(move |_btn| {
        let p = panel_for_check.clone();
        let updates_view = updates_view_for_check.clone();
        let check_btn = check_btn_for_check.clone();
        let install_btn = install_btn_for_check.clone();
        p.set_badge("Checking…");
        p.set_busy(true);
        updates_view.set_visible(false);

        // spawn_future_local pins this future to the main thread's GLib
        // context (glib's equivalent of tokio::task::spawn_local), so it
        // can freely capture p/updates_view/etc (not Send). Only the
        // actual pacman call crosses onto the Tokio runtime's blocking
        // pool via spawn_blocking, and .await resumes back here on the
        // main thread once it's done -- no idle_add_once / channel needed.
        glib::spawn_future_local(async move {
            let updates = tokio_runtime()
                .spawn_blocking(move || check_updates(filter))
                .await
                .unwrap_or_default();
            let count = updates.len();
            let lines: String = updates
                .iter()
                .map(|(n, c, v)| format!("  {n}   {c} -> {v}"))
                .collect::<Vec<_>>()
                .join("\n");
            if count > 0 {
                let label = if count > 1 { "s" } else { "" };
                p.set_badge(&format!("{count} update{label} available"));
                p.updates_view.buffer().set_text(&lines);
                updates_view.set_visible(true);
                install_btn.set_sensitive(true);
            } else {
                p.set_badge("Up to date");
                install_btn.set_sensitive(false);
            }
            p.set_busy(false);
            check_btn.set_sensitive(true);
        });
    });

    // Install handler. cfg.install_cmd is &'static [&'static str] (Copy,
    // Send) -- no cloning needed. Everything below runs as a single local
    // future: the child process and its stdout/stderr are driven by
    // Tokio (entered once in main()), and every GTK widget touch happens
    // right here on the main thread, so none of it needs to be Send.
    let cmd = cfg.install_cmd;
    let panel_for_install = panel.clone();
    install_btn.connect_clicked(move |_btn| {
        let p = panel_for_install.clone();
        p.set_badge("Installing…");
        p.set_busy(true);
        p.log_view.set_visible(true);
        p.log_view.buffer().set_text("");

        glib::spawn_future_local(async move {
            let mut child = match tokio::process::Command::new(cmd[0])
                .args(&cmd[1..])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    let buf = p.log_view.buffer();
                    let mut end = buf.end_iter();
                    buf.insert(&mut end, &format!("\n[ERROR] failed to spawn: {e}\n"));
                    p.set_badge("Failed");
                    p.progress.set_visible(false);
                    p.install_btn.set_sensitive(true);
                    p.check_btn.set_sensitive(true);
                    p.cancel_btn.set_sensitive(false);
                    return;
                }
            };

            *p.running_pid.lock().unwrap() = child.id();

            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let p_out = p.clone();
            let out_fut = async move {
                if let Some(out) = stdout {
                    let mut lines = tokio::io::BufReader::new(out).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let buf = p_out.log_view.buffer();
                        let mut end = buf.end_iter();
                        buf.insert(&mut end, &format!("{line}\n"));
                    }
                }
            };
            let p_err = p.clone();
            let err_fut = async move {
                if let Some(err) = stderr {
                    let mut lines = tokio::io::BufReader::new(err).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let buf = p_err.log_view.buffer();
                        let mut end = buf.end_iter();
                        buf.insert(&mut end, &format!("{line}\n"));
                    }
                }
            };
            // Drains both streams concurrently (so interleaved pacman
            // output shows up live) and waits for exit, all as one
            // cooperatively-scheduled local future -- no OS threads.
            let (_, _, status) = tokio::join!(out_fut, err_fut, child.wait());
            let code = status.ok().and_then(|s| s.code()).unwrap_or(1);
            *p.running_pid.lock().unwrap() = None;

            if code == 0 {
                p.set_badge("Done");
            } else {
                p.set_badge(&format!("Failed (code {code})"));
            }
            p.progress.set_visible(false);
            p.install_btn.set_sensitive(true);
            p.check_btn.set_sensitive(true);
            p.cancel_btn.set_sensitive(false);
        });
    });

    let panel_for_cancel = panel.clone();
    cancel_btn.connect_clicked(move |_btn| {
        let p = &panel_for_cancel;
        let pid = *p.running_pid.lock().unwrap();
        if let Some(pid) = pid {
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

    // Entered once, kept alive for the process lifetime (the guard is
    // never dropped): every glib::spawn_future_local task below relies on
    // a Tokio runtime already running so tokio::process / spawn_blocking
    // work when awaited from the GTK main loop.
    let rt = tokio_runtime();
    let _rt_guard = rt.enter();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        apply_global_style();
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Zohara Updates Center")
            .default_width(780)
            .default_height(640)
            .build();

        let root = Box::new(Orientation::Vertical, 18);
        root.set_margin_top(24);
        root.set_margin_bottom(24);
        root.set_margin_start(28);
        root.set_margin_end(28);

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

        let div = gtk::Separator::new(Orientation::Horizontal);
        root.append(&div);

        let scrolled = ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let panel_box = Box::new(Orientation::Vertical, 16);
        scrolled.set_child(Some(&panel_box));
        root.append(&scrolled);

        for cfg in &PANELS {
            let (card, _panel) = build_panel(cfg);
            panel_box.append(&card);
        }

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
    });

    let _ = app.run_with_args::<&str>(&[]);
    ExitCode::SUCCESS
}
