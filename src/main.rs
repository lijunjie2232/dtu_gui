//! dtu desktop client: a native window (egui). It *is* the app: the core listener,
//! discovery and REST surface start with it and stop when it exits.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dtu_core::proto::{self, InviteView};
use dtu_core::state::{Decision, State};
use dtu_core::trust::Store;
use dtu_core::{cfg, discover, embed, guard, identity, invite};
use eframe::egui::{self, Align, Color32, Grid, Layout, RichText, ScrollArea, TextEdit};

const TABS: [&str; 4] = ["Status", "Devices", "Invitations", "Code"];
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
    note: String,
}

enum Cmd {
    Refresh,
    Scan,
    NewCode,
    Accept(String),
    Deny(String),
}

async fn snapshot(st: &Arc<State>) -> View {
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
    }
}

/// Own thread, republishing a fresh `View` every second: the window never awaits.
fn spawn_worker(st: Arc<State>, view: Arc<Mutex<View>>) -> Sender<Cmd> {
    let (tx, rx) = channel::<Cmd>();
    std::thread::spawn(move || {
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
            let mut fresh = rt.block_on(snapshot(&st));
            match note {
                Ok(msg) if !msg.is_empty() => fresh.note = msg,
                Ok(_) => {}
                Err(e) => fresh.note = e,
            }
            *view.lock().unwrap() = fresh;
        }
    });
    tx
}

struct App {
    state: Arc<State>,
    view: Arc<Mutex<View>>,
    tx: Sender<Cmd>,
    tab: usize,
    /// Device row picked on the Devices tab; its alias is edited next to the table.
    selected: Option<String>,
    alias: String,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_secs(1));
        let view = self.view.lock().expect("view lock").clone();
        let api = dtu_core::api::api_port(&self.state);

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
                    _ => self.code_tab(ui, &view),
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
            let note = match store.set_alias(&id, Some(&alias)).and_then(|_| store.save()) {
                Ok(()) => format!("{id} is now {alias}"),
                Err(e) => e,
            };
            view.lock().unwrap().note = note;
        });
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
    let tx = spawn_worker(daemon.state.clone(), view.clone());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([880.0, 600.0]),
        ..Default::default()
    };
    let app = App { state: daemon.state.clone(), view, tx, tab: 0, alias: String::new() };
    let out = eframe::run_native("dtu", options, Box::new(move |_cc| Ok(Box::new(app))));
    // Closing the window stops the services this process was hosting.
    daemon.stop();
    out
}
