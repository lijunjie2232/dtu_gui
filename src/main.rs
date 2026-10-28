//! dtu desktop client: a native window (egui). It *is* the app: the core listener,
//! discovery and REST surface start with it and stop when it exits.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dtu_core::proto::{self, InviteView};
use dtu_core::state::{Decision, State};
use dtu_core::trust::Store;
use dtu_core::{cfg, discover, embed, guard, identity, invite};
use eframe::egui::{self, Align, Color32, Grid, Layout, RichText, ScrollArea, TextEdit};
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const TABS: [&str; 6] = ["Status", "Devices", "Invitations", "Code", "Log", "Settings"];
/// What the log tab shows; `None` is everything.
const FILTERS: [(&str, Option<&str>); 4] =
    [("All", None), ("Auth", Some("auth")), ("Files", Some("file")), ("Invites", Some("invite"))];
const ACCENT: Color32 = Color32::from_rgb(59, 130, 246);
const WARN: Color32 = Color32::from_rgb(230, 160, 40);
const QUIET: Color32 = Color32::from_gray(140);

/// One trusted device, ready to draw as a table row.
#[derive(Clone)]
struct DeviceRow {
    id: String,
    alias: Option<String>,
    addr: Option<String>,
    expires_at: u64,
    live: bool,
}

#[derive(Default, Clone)]
struct View {
    status: Vec<(String, String)>,
    devices: Vec<DeviceRow>,
    invites: Vec<InviteView>,
    code: String,
    password: bool,
    consumed: bool,
    /// Audit log: who authenticated, and which files moved.
    logs: Vec<dtu_core::audit::Entry>,
    /// Every setting as (key, current value), straight from config.json.
    settings: Vec<(String, String)>,
    note: String,
}

enum Cmd {
    Refresh,
    Scan,
    NewCode,
    Accept(String),
    Deny(String),
    SetConfig { key: String, value: String },
}

async fn snapshot(st: &Arc<State>, log_lines: usize) -> View {
    let trusted = st.trust.lock().await.devices.len();
    let pending = st.invites.lock().await.len();
    let mut v = View {
        status: vec![
            ("device".into(), st.id.id.clone()),
            (
                "version".into(),
                format!("{} (proto v{})", dtu_core::VERSION, proto::PROTO),
            ),
            ("listen".into(), format!("0.0.0.0:{}", st.cfg.port)),
            ("root".into(), guard::for_display(&st.root)),
            ("unlimited".into(), st.cfg.unlimit.to_string()),
            ("trusted".into(), trusted.to_string()),
            ("pending".into(), pending.to_string()),
            (
                "uptime".into(),
                format!("{}s", identity::now().saturating_sub(st.started)),
            ),
        ],
        ..Default::default()
    };

    let store: Store = st.trust.lock().await.clone();
    for (id, d) in &store.devices {
        v.devices.push(DeviceRow {
            id: id.clone(),
            alias: d.alias.clone(),
            addr: d.addr.clone(),
            expires_at: d.expires_at,
            live: store.get(id).is_some(),
        });
    }
    v.invites = invite::list(st).await;
    let c = st.code.lock().await;
    v.code = c.code.clone();
    v.password = c.password.is_some();
    v.consumed = c.consumed;
    drop(c);
    v.logs = dtu_core::audit::entries(None, log_lines);
    let cfg = dtu_core::cfg::Config::load();
    v.settings = dtu_core::cfg::fields()
        .iter()
        .map(|f| (f.key.to_string(), cfg.get(f.key).unwrap_or_default()))
        .collect();
    v
}

async fn act(st: &Arc<State>, cmd: Cmd) -> Result<String, String> {
    match cmd {
        Cmd::Refresh => Ok(String::new()),
        Cmd::Scan => {
            let trust = st.trust.lock().await.clone();
            let items = discover::scan(st.cfg.port, 1500, &st.id.id, &trust).await;
            if items.is_empty() {
                return Ok("no device answered the LAN probe".into());
            }
            let found: Vec<String> = items
                .iter()
                .map(|s| format!("{}{}", s.id, if s.trusted { " (trusted)" } else { "" }))
                .collect();
            Ok(format!("{} device(s): {}", found.len(), found.join(", ")))
        }
        Cmd::NewCode => {
            st.rotate_code(None).await;
            let c = st.code.lock().await;
            Ok(format!("new code {}", c.code))
        }
        Cmd::Accept(id) => {
            let expires_at = cfg::parse_expiry("30d")?;
            invite::decide(st, &id, Decision::Accept { expires_at }).await?;
            Ok(format!("accepted invite {id}"))
        }
        Cmd::Deny(id) => {
            invite::decide(st, &id, Decision::Deny).await?;
            Ok(format!("denied invite {id}"))
        }
        Cmd::SetConfig { key, value } => {
            let mut cfg = dtu_core::cfg::Config::load();
            cfg.set(&key, &value)?;
            cfg.save().map_err(|e| format!("save config: {e}"))?;
            if dtu_core::cfg::applies_live(&key) {
                st.apply_live(&cfg).await;
            }
            Ok(format!("{key} = {value}"))
        }
    }
}

/// Own thread, republishing a fresh `View` every second: the window never awaits.
fn spawn_worker(st: Arc<State>, view: Arc<Mutex<View>>, log_lines: usize) -> (Sender<Cmd>, std::thread::JoinHandle<()>) {
    let (tx, rx) = channel::<Cmd>();
    let handle = std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                view.lock().unwrap().note = format!("runtime: {e}");
                return;
            }
        };
        loop {
            let cmd = match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(cmd) => cmd,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Cmd::Refresh,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            };
            let note = rt.block_on(act(&st, cmd));
            let mut fresh = rt.block_on(snapshot(&st, log_lines));
            match note {
                Ok(msg) if !msg.is_empty() => fresh.note = msg,
                Ok(_) => {}
                Err(e) => fresh.note = e,
            }
            *view.lock().unwrap() = fresh;
        }
    });
    (tx, handle)
}

struct App {
    state: Arc<State>,
    view: Arc<Mutex<View>>,
    tx: Sender<Cmd>,
    tab: usize,
    /// Device row picked on the Devices tab; its alias is edited next to the table.
    selected: Option<String>,
    alias: String,
    /// Index into `FILTERS`, used by the Log tab.
    filter: usize,
    /// Settings being edited, keyed by config key (initialised from `view.settings`).
    edits: HashMap<String, String>,
    settings_loaded: bool,
    /// Repaint interval, from `ui_refresh_secs` at startup.
    refresh: Duration,
    /// Kept alive for the life of the app; dropping it removes the icon.
    tray: Option<TrayIcon>,
    /// Tray menu item ids, for polling events.
    tray_show_id: Option<tray_icon::menu::MenuId>,
    tray_settings_id: Option<tray_icon::menu::MenuId>,
    tray_exit_id: Option<tray_icon::menu::MenuId>,
    /// Hosted core daemon, kept here so settings can restart it.
    daemon: Option<embed::Daemon>,
    /// Worker thread handle.
    worker_handle: Option<std::thread::JoinHandle<()>>,
    /// Set by the worker when a restart-requiring setting changed.
    restart_pending: Arc<AtomicBool>,
    /// Debounced restart for text fields that need it.
    restart_needed: bool,
    last_restart_change: Option<std::time::Instant>,
    /// What the tray asked for: 1 show, 2 show settings, 3 exit.
    tray_cmd: Arc<AtomicU8>,
    /// Set when the user really wants to quit (tray Exit), so close is not hijacked.
    exiting: bool,
    /// Window is parked in the tray: keep the frame loop breathing so a tray click
    /// is picked up even if a hidden window gets no paint events.
    hidden: bool,
}

/// Tray menu and left-click both land here: a small command byte plus a repaint.
fn build_tray(_ctx: &egui::Context, _cmd: &Arc<AtomicU8>) -> (Option<TrayIcon>, tray_icon::menu::MenuId, tray_icon::menu::MenuId, tray_icon::menu::MenuId) {
    let menu = Menu::new();
    let show = MenuItem::new("Show dtu", true, None);
    let settings = MenuItem::new("Settings", true, None);
    let exit = MenuItem::new("Exit", true, None);
    if let Err(e) = menu.append_items(&[&show, &settings, &PredefinedMenuItem::separator(), &exit]) {
        eprintln!("build_tray: append_items failed: {e}");
        return (None, show.id().clone(), settings.id().clone(), exit.id().clone());
    }
    let (show_id, settings_id, exit_id) = (show.id().clone(), settings.id().clone(), exit.id().clone());
    eprintln!("ids: show={} settings={} exit={}", show_id.0, settings_id.0, exit_id.0);

    eprintln!("build_tray: building");
    let t = TrayIconBuilder::new()
        .with_tooltip("dtu")
        .with_icon(tray_image())
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build();
    eprintln!("build_tray: result is_some={}", t.is_ok());
    (t.ok(), show_id, settings_id, exit_id)
}

/// 32x32 RGBA drawn in code: a blue tile with a white border and diagonal, so the
/// binary needs no icon asset.
fn tray_image() -> tray_icon::Icon {
    const N: u32 = 32;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let border = x < 2 || y < 2 || x >= N - 2 || y >= N - 2;
            let diagonal = (x as i32 - y as i32).abs() < 6;
            rgba.extend_from_slice(if border || diagonal {
                &[255, 255, 255, 255]
            } else {
                &[59, 130, 246, 255]
            });
        }
    }
    tray_icon::Icon::from_rgba(rgba, N, N).expect("32x32 rgba icon")
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.restart_needed {
            if let Some(t) = self.last_restart_change {
                if t.elapsed() >= Duration::from_secs(1) {
                    self.restart_needed = false;
                    self.last_restart_change = None;
                    self.restart_pending.store(true, Ordering::SeqCst);
                } else {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        if self.restart_pending.swap(false, Ordering::SeqCst) {
            self.restart_core();
        }
        ctx.request_repaint_after(self.refresh);
        let view = self.view.lock().expect("view lock").clone();
        let api = dtu_core::api::api_port(&self.state);

        // Tray: show the window, open Settings, or really quit.
        // Poll tray events (menu + left click) since we no longer use global handlers.
        if let Some(show_id) = &self.tray_show_id {
            while let Ok(e) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                let what = if e.id == *show_id {
                    1
                } else if Some(&e.id) == self.tray_settings_id.as_ref() {
                    2
                } else if Some(&e.id) == self.tray_exit_id.as_ref() {
                    3
                } else {
                    0
                };
                if what != 0 {
                    self.tray_cmd.store(what, Ordering::SeqCst);
                }
            }
        }
        while let Ok(e) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                self.tray_cmd.store(1, Ordering::SeqCst);
            }
        }
        match self.tray_cmd.swap(0, Ordering::SeqCst) {
            1 => {
                eprintln!("tray_cmd=1 show");
                self.hidden = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            2 => {
                self.hidden = false;
                self.tab = 5;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            3 => {
                self.exiting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
        if self.hidden {
            ctx.request_repaint_after(Duration::from_millis(400));
        }

        // Closing the window means "hide to the tray" unless the user asked for a real exit.
        let close_to_tray = view
            .settings
            .iter()
            .find(|(k, _)| k == "ui_close_to_tray")
            .map(|(_, v)| v == "true")
            .unwrap_or(true);
        if !self.exiting && ctx.input(|i| i.viewport().close_requested()) && close_to_tray && self.tray.is_some() {
            self.hidden = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        egui::TopBottomPanel::top("bar").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("dtu").size(20.0).strong().color(ACCENT));
                ui.add_space(8.0);
                ui.separator();
                for (i, name) in TABS.iter().enumerate() {
                    if ui.selectable_label(self.tab == i, *name).clicked() {
                        self.tab = i;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Refresh").clicked() {
                        let _ = self.tx.send(Cmd::Refresh);
                    }
                    if ui.button("Scan LAN").clicked() {
                        let _ = self.tx.send(Cmd::Scan);
                    }
                    if !view.invites.is_empty() {
                        ui.label(
                            RichText::new(format!("{} pending", view.invites.len())).color(WARN),
                        );
                    }
                });
            });
            ui.add_space(6.0);
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(short_id(&self.state.id.id))
                        .monospace()
                        .color(QUIET),
                );
                ui.label(RichText::new("·").color(QUIET));
                ui.label(RichText::new(format!("listen {}", self.state.cfg.port)).color(QUIET));
                ui.label(RichText::new("·").color(QUIET));
                ui.label(RichText::new(format!("api 127.0.0.1:{api}")).color(QUIET));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if view.note.is_empty() {
                        ui.label(RichText::new("ready").color(QUIET));
                    } else {
                        ui.label(view.note.as_str());
                    }
                });
            });
            ui.add_space(2.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(ui, |ui| match self.tab {
                    0 => self.status_tab(ui, &view, api),
                    1 => self.devices_tab(ui, &view),
                    2 => self.invites_tab(ui, &view),
                    3 => self.code_tab(ui, &view),
                    4 => self.log_tab(ui, &view),
                    _ => self.settings_tab(ui, &view),
                });
        });
    }
}

impl App {
    fn status_tab(&mut self, ui: &mut egui::Ui, view: &View, api: u16) {
        ui.columns(2, |cols| {
            cols[0].label(RichText::new("This device").strong().size(15.0));
            cols[0].add_space(6.0);
            Grid::new("status")
                .num_columns(2)
                .striped(true)
                .spacing([18.0, 7.0])
                .show(&mut cols[0], |ui| {
                    for (key, value) in &view.status {
                        ui.label(RichText::new(key).color(QUIET));
                        ui.label(value);
                        ui.end_row();
                    }
                });

            let col = &mut cols[1];
            col.label(RichText::new("Third-party API").strong().size(15.0));
            col.add_space(6.0);
            col.label(RichText::new("Loopback REST for any frontend.").color(QUIET));
            col.add_space(6.0);
            col.horizontal(|ui| {
                ui.monospace(format!("http://127.0.0.1:{api}"));
                if ui.button("copy key").clicked() {
                    let key = std::fs::read_to_string(cfg::file("control.token"))
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    ui.output_mut(|o| o.copied_text = key);
                }
            });
            col.add_space(14.0);
            col.label(RichText::new("Endpoints").strong());
            for (route, what) in [
                ("GET  /api/status", "device, ports, counters"),
                ("GET  /api/devices", "trusted devices"),
                ("GET  /api/invites", "pending invitations"),
                ("POST /api/transfer", "push / pull / diff / sync"),
                ("GET  /api/transfer/<id>", "transfer progress"),
            ] {
                col.horizontal(|ui| {
                    ui.monospace(route);
                    ui.label(RichText::new(what).color(QUIET).small());
                });
            }
        });
    }

    fn devices_tab(&mut self, ui: &mut egui::Ui, view: &View) {
        if view.devices.is_empty() {
            // sit the text a third of the way down instead of at the very top
            ui.add_space((ui.available_height() * 0.3).min(180.0));
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No trusted devices yet").strong().size(16.0));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Pair one from the Invitations tab, or run `dtu device add <host:port>` \
                         on this machine.",
                    )
                    .color(QUIET),
                );
            });
            return;
        }

        Grid::new("devices")
            .num_columns(5)
            .striped(true)
            .spacing([16.0, 7.0])
            .show(ui, |ui| {
                for h in ["device", "alias", "address", "expires", ""] {
                    ui.label(RichText::new(h).color(QUIET).strong());
                }
                ui.end_row();

                for row in &view.devices {
                    let picked = self.selected.as_deref() == Some(row.id.as_str());
                    if ui
                        .selectable_label(picked, RichText::new(&row.id).monospace())
                        .on_hover_text("click to select")
                        .clicked()
                    {
                        self.selected = Some(row.id.clone());
                        self.alias = row.alias.clone().unwrap_or_default();
                    }
                    ui.label(row.alias.as_deref().unwrap_or("—"));
                    ui.label(RichText::new(row.addr.as_deref().unwrap_or("—")).monospace());
                    if !row.live {
                        ui.label(RichText::new("expired").color(WARN));
                    } else if row.expires_at == 0 {
                        ui.label("never");
                    } else {
                        ui.label(dtu_core::audit::iso(row.expires_at));
                    }
                    if ui.small_button("forget").clicked() {
                        self.forget(&row.id);
                    }
                    ui.end_row();
                }
            });

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(4.0);
        ui.horizontal(|ui| match self.selected.clone() {
            None => {
                ui.label(RichText::new("Select a device to rename or forget it.").color(QUIET));
            }
            Some(id) => {
                ui.label(RichText::new(short_id(&id)).monospace());
                ui.label(RichText::new("alias").color(QUIET));
                let edit = ui.add(TextEdit::singleline(&mut self.alias).desired_width(180.0));
                let save = ui.button("save").clicked()
                    || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                if save {
                    self.rename(&id, &self.alias.clone());
                }
                if ui.button("forget").clicked() {
                    self.forget(&id);
                }
                if ui.button("clear").clicked() {
                    self.selected = None;
                    self.alias.clear();
                }
            }
        });
    }

    fn invites_tab(&mut self, ui: &mut egui::Ui, view: &View) {
        if view.invites.is_empty() {
            // sit the text a third of the way down instead of at the very top
            ui.add_space((ui.available_height() * 0.3).min(180.0));
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No pending invitations").strong().size(16.0));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "An invitation arrives when another device wants to trust this one. \
                         It waits here until accepted or denied.",
                    )
                    .color(QUIET),
                );
            });
            return;
        }

        ScrollArea::vertical().show(ui, |ui| {
            for inv in &view.invites {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&inv.id).monospace().strong().size(15.0));
                        ui.label(format!("from {}", inv.from));
                        ui.label(RichText::new(&inv.addr).monospace().color(QUIET));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("deny").clicked() {
                                let _ = self.tx.send(Cmd::Deny(inv.id.clone()));
                            }
                            if ui.button("accept 30d").clicked() {
                                let _ = self.tx.send(Cmd::Accept(inv.id.clone()));
                            }
                        });
                    });
                    ui.label(
                        RichText::new(format!(
                            "requested expiry {}",
                            if inv.expires_at == 0 {
                                "never".to_string()
                            } else {
                                dtu_core::audit::iso(inv.expires_at)
                            }
                        ))
                        .color(QUIET)
                        .small(),
                    );
                });
                ui.add_space(4.0);
            }
        });
    }

    fn code_tab(&mut self, ui: &mut egui::Ui, view: &View) {
        ui.label(RichText::new("One-time pairing code").strong().size(15.0));
        ui.add_space(8.0);
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&view.code)
                            .monospace()
                            .size(34.0)
                            .strong()
                            .color(ACCENT),
                    );
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.add_space(6.0);
                        if ui.button("new code").clicked() {
                            let _ = self.tx.send(Cmd::NewCode);
                        }
                        if ui.button("copy").clicked() {
                            ui.output_mut(|o| o.copied_text = view.code.clone());
                        }
                    });
                });
                if view.consumed {
                    ui.label(RichText::new("used by one transfer — refresh to reuse").color(WARN));
                }
                if view.password {
                    ui.label("a password is required on top of the code");
                }
            });
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Share it with the person sending files: they run \
                 `dtu push <this host> <file> --code <code>` and pair once.",
            )
            .color(QUIET),
        );
    }

    /// Who authenticated, and which files they moved. Filtering happens client-side
    /// so the one background fetch serves every view.
    fn log_tab(&mut self, ui: &mut egui::Ui, view: &View) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Audit log").strong().size(15.0));
            ui.add_space(8.0);
            egui::ComboBox::from_id_salt("log-filter")
                .selected_text(FILTERS[self.filter.min(FILTERS.len() - 1)].0)
                .show_ui(ui, |ui| {
                    for (i, (name, _)) in FILTERS.iter().enumerate() {
                        ui.selectable_value(&mut self.filter, i, *name);
                    }
                });
            ui.label(
                RichText::new("auth decisions and every file that moved, with the peer and its address")
                    .color(QUIET)
                    .small(),
            );
        });
        ui.add_space(6.0);

        let keep = FILTERS[self.filter.min(FILTERS.len() - 1)].1;
        let rows: Vec<&dtu_core::audit::Entry> = view
            .logs
            .iter()
            .filter(|e| keep.is_none_or(|k| e.event == k))
            .collect();
        if rows.is_empty() {
            ui.label(RichText::new("nothing logged yet").color(QUIET));
            return;
        }
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Grid::new("log")
                    .num_columns(5)
                    .striped(true)
                    .spacing([14.0, 5.0])
                    .show(ui, |ui| {
                        for h in ["time", "event", "result", "peer", "detail"] {
                            ui.label(RichText::new(h).color(QUIET).strong());
                        }
                        ui.end_row();
                        for e in rows {
                            let time = e
                                .ts
                                .split('T')
                                .nth(1)
                                .unwrap_or(&e.ts)
                                .trim_end_matches('Z');
                            ui.monospace(time);
                            ui.label(match e.event.as_str() {
                                "file" => RichText::new("file").color(ACCENT),
                                "auth" => RichText::new("auth"),
                                "invite" => RichText::new("invite").color(WARN),
                                other => RichText::new(other).color(QUIET),
                            });
                            ui.label(match e.result.as_str() {
                                "ok" => RichText::new("ok").color(Color32::from_rgb(64, 160, 96)),
                                "deny" | "error" => RichText::new(e.result.as_str()).color(WARN),
                                other => RichText::new(other).color(QUIET),
                            });
                            let who = e.device.as_deref().unwrap_or("-");
                            ui.label(RichText::new(format!("{} · {}", short_id(who), e.addr)).monospace().small());

                            let mut detail = String::new();
                            if let Some(p) = &e.path {
                                detail.push_str(p);
                            }
                            if let Some(b) = e.bytes {
                                if b > 0 {
                                    detail.push_str(&format!("  {}", dtu_core::audit::human_bytes(b)));
                                }
                            }
                            if !detail.is_empty() {
                                detail.push(' ');
                            }
                            detail.push_str(&e.method);
                            if let Some(r) = &e.reason {
                                detail.push_str(&format!(" — {r}"));
                            }
                            ui.label(RichText::new(detail).small());
                            ui.end_row();
                        }
                    });
            });
    }

    /// Every setting; changes are saved immediately and restart the core when needed.
    fn settings_tab(&mut self, ui: &mut egui::Ui, view: &View) {
        if !self.settings_loaded && !view.settings.is_empty() {
            // "null" is how an unset optional looks in JSON; show it as an empty box
            self.edits = view
                .settings
                .iter()
                .map(|(k, v)| (k.clone(), if v == "null" { String::new() } else { v.clone() }))
                .collect();
            self.settings_loaded = true;
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").strong().size(15.0));
            ui.add_space(8.0);
            if ui.button("Reload").clicked() {
                self.settings_loaded = false;
            }
            ui.label(RichText::new("saved to config.json").color(QUIET).small());
        });
        ui.add_space(6.0);

        // Value column width follows the longest value (clamped), so every row
        // shares one width instead of each TextEdit sizing itself.
        let font_id = egui::TextStyle::Body.resolve(ui.style());
        let mut value_width = 0.0_f32;
        for f in dtu_core::cfg::fields() {
            if f.kind == cfg::Kind::Bool {
                continue;
            }
            let v = self
                .edits
                .get(f.key)
                .cloned()
                .or_else(|| view.settings.iter().find(|(k, _)| k == f.key).map(|(_, v)| if v == "null" { String::new() } else { v.clone() }))
                .unwrap_or_default();
            let w = ui.fonts(|fonts| fonts.layout_no_wrap(v, font_id.clone(), egui::Color32::WHITE).size().x);
            value_width = value_width.max(w);
        }
        let header_w = ui.fonts(|fonts| fonts.layout_no_wrap("value".to_string(), font_id.clone(), egui::Color32::WHITE).size().x);
        value_width = (value_width.max(header_w) + 16.0).clamp(320.0, 640.0);

        Grid::new("settings")
            .num_columns(3)
            .striped(true)
            .max_col_width(1000.0)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                for h in ["key", "value", "help"] {
                    ui.label(RichText::new(h).color(QUIET).strong());
                }
                ui.end_row();

                for f in dtu_core::cfg::fields() {
                    let value = self
                        .edits
                        .entry(f.key.to_string())
                        .or_insert_with(|| view.settings.iter().find(|(k, _)| k == f.key).map(|(_, v)| v.clone()).unwrap_or_default());
                    ui.monospace(f.key);
                    let changed = match f.kind {
                        cfg::Kind::Bool => {
                            let mut on = value.trim() == "true";
                            let c = ui.checkbox(&mut on, "").changed();
                            if c {
                                *value = on.to_string();
                                if f.restart {
                                    self.restart_pending.store(true, Ordering::SeqCst);
                                }
                            }
                            c
                        }
                        _ => {
                            let resp = ui.add(TextEdit::singleline(value).desired_width(value_width));
                            let c = resp.changed();
                            if c && f.restart {
                                self.restart_needed = true;
                                self.last_restart_change = Some(std::time::Instant::now());
                            }
                            c
                        }
                    };
                    if changed {
                        let _ = self.tx.send(Cmd::SetConfig {
                            key: f.key.to_string(),
                            value: value.clone(),
                        });
                    }
                    ui.label(RichText::new(f.help).color(QUIET).small());
                    ui.end_row();
                }
            });
    }

    /// Edits land in the live store, so the running listener sees them immediately.
    fn forget(&mut self, id: &str) {
        let st = self.state.clone();
        let view = self.view.clone();
        let id = id.to_string();
        if self.selected.as_deref() == Some(id.as_str()) {
            self.selected = None;
        }
        std::thread::spawn(move || {
            let mut store = st.trust.blocking_lock();
            let note = if store.remove(&id) {
                store
                    .save()
                    .map(|_| format!("forgot {id}"))
                    .unwrap_or_else(|e| e)
            } else {
                format!("no such device: {id}")
            };
            view.lock().unwrap().note = note;
        });
    }

    fn rename(&mut self, id: &str, alias: &str) {
        let st = self.state.clone();
        let view = self.view.clone();
        let (id, alias) = (id.to_string(), alias.to_string());
        std::thread::spawn(move || {
            let mut store = st.trust.blocking_lock();
            let note = match store
                .set_alias(&id, Some(&alias))
                .and_then(|_| store.save())
            {
                Ok(()) => format!("{id} is now {alias}"),
                Err(e) => e,
            };
            view.lock().unwrap().note = note;
        });
    }

    fn restart_core(&mut self) {
        // Stop the worker first.
        let (dummy_tx, _dummy_rx) = channel::<Cmd>();
        let old_tx = std::mem::replace(&mut self.tx, dummy_tx);
        drop(old_tx);
        if let Some(h) = self.worker_handle.take() {
            let _ = h.join();
        }
        // Stop the current core.
        if let Some(d) = self.daemon.take() {
            d.stop();
        }
        // Host a fresh core with the updated config.
        let daemon = match embed::host(cfg::Config::load()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("dtu: restart failed: {e}");
                return;
            }
        };
        if let Err(e) = daemon.wait_ready() {
            eprintln!("dtu: restart not ready: {e}");
            daemon.stop();
            return;
        }
        let view = Arc::new(Mutex::new(View::default()));
        let startup = daemon.state.cfg.clone();
        let repaint = Duration::from_secs(if startup.ui_refresh_secs > 0 { startup.ui_refresh_secs } else { 1 });
        let log_lines = if startup.ui_log_lines > 0 { startup.ui_log_lines } else { 500 };
        let (tx, handle) = spawn_worker(daemon.state.clone(), view.clone(), log_lines);
        self.state = daemon.state.clone();
        self.view = view;
        self.tx = tx;
        self.worker_handle = Some(handle);
        self.daemon = Some(daemon);
        self.edits.clear();
        self.settings_loaded = false;
        self.refresh = repaint;
        self.restart_needed = false;
        self.last_restart_change = None;
    }
}

/// Device ids are long; the status bar only needs enough to tell them apart.
fn short_id(id: &str) -> String {
    if id.len() <= 12 {
        id.to_string()
    } else {
        format!("{}…", &id[..12])
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let (dummy_tx, _dummy_rx) = channel::<Cmd>();
        let old_tx = std::mem::replace(&mut self.tx, dummy_tx);
        drop(old_tx);
        if let Some(h) = self.worker_handle.take() {
            let _ = h.join();
        }
        if let Some(d) = self.daemon.take() {
            d.stop();
        }
    }
}

fn main() -> eframe::Result<()> {
    // This process is the app *and* the server.
    let daemon = match embed::host(cfg::Config::load()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("dtu: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = daemon.wait_ready() {
        eprintln!("dtu: {e}");
        daemon.stop();
        std::process::exit(1);
    }

    let view = Arc::new(Mutex::new(View::default()));
    let startup = daemon.state.cfg.clone();
    let repaint = Duration::from_secs(if startup.ui_refresh_secs > 0 { startup.ui_refresh_secs } else { 1 });
    let log_lines = if startup.ui_log_lines > 0 { startup.ui_log_lines } else { 500 };
    let restart_pending = Arc::new(AtomicBool::new(false));
    let (tx, worker_handle) = spawn_worker(daemon.state.clone(), view.clone(), log_lines);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 620.0])
            .with_min_inner_size([680.0, 460.0]),
        ..Default::default()
    };
    let app = App {
        state: daemon.state.clone(),
        view,
        tx,
        tab: 0,
        selected: None,
        alias: String::new(),
        filter: 0,
        edits: HashMap::new(),
        settings_loaded: false,
        refresh: repaint,
        tray: None,
        tray_show_id: None,
        tray_settings_id: None,
        tray_exit_id: None,
        tray_cmd: Arc::new(AtomicU8::new(0)),
        exiting: false,
        hidden: false,
        daemon: Some(daemon),
        worker_handle: Some(worker_handle),
        restart_pending,
        restart_needed: false,
        last_restart_change: None,
    };
    let out = eframe::run_native(
        "dtu",
        options,
        Box::new(move |cc| {
            // The tray needs the egui context to wake the window on a click.
            let mut app = app;
            let (tray, show_id, settings_id, exit_id) = build_tray(&cc.egui_ctx, &app.tray_cmd.clone());
            app.tray = tray;
            if app.tray.is_none() {
                eprintln!("dtu: no tray available; closing the window will quit");
            }
            app.tray_show_id = Some(show_id);
            app.tray_settings_id = Some(settings_id);
            app.tray_exit_id = Some(exit_id);
            Ok(Box::new(app))
        }),
    );
    out
}
