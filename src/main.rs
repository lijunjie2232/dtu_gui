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
    alias: String,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_secs(1));
        let view = self.view.lock().expect("view lock").clone();

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in TABS.iter().enumerate() {
                    if ui.selectable_label(self.tab == i, *name).clicked() {
                        self.tab = i;
                    }
                }
                ui.separator();
                if ui.button("Scan LAN").clicked() {
                    let _ = self.tx.send(Cmd::Scan);
                }
                if ui.button("Refresh").clicked() {
                    let _ = self.tx.send(Cmd::Refresh);
                }
            });
        });

        egui::TopBottomPanel::bottom("note").show(ctx, |ui| {
            ui.label(if view.note.is_empty() { "ready" } else { view.note.as_str() });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            0 => {
                egui::Grid::new("status").num_columns(2).striped(true).show(ui, |ui| {
                    for (key, value) in &view.status {
                        ui.label(key);
                        ui.label(value);
                        ui.end_row();
                    }
                });
            }
            1 => {
                let mut forget = None;
                for line in &view.devices {
                    ui.horizontal(|ui| {
                        ui.label(line);
                        if ui.small_button("forget").clicked() {
                            forget = Some(line.split_whitespace().next().unwrap_or_default().to_string());
                        }
                    });
                }
                if let Some(id) = forget {
                    self.forget(&id);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("alias:");
                    ui.text_edit_singleline(&mut self.alias);
                    if ui.button("rename first device").clicked() {
                        let alias = self.alias.clone();
                        if let Some(id) = view.devices.first().and_then(|l| l.split_whitespace().next()) {
                            self.rename(id, &alias);
                        }
                    }
                });
            }
            2 => {
                if view.invites.is_empty() {
                    ui.label("no pending invitations");
                }
                for inv in &view.invites {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} from {} at {}", inv.id, inv.from, inv.addr));
                        if ui.button("accept").clicked() {
                            let _ = self.tx.send(Cmd::Accept(inv.id.clone()));
                        }
                        if ui.button("deny").clicked() {
                            let _ = self.tx.send(Cmd::Deny(inv.id.clone()));
                        }
                    });
                }
            }
            _ => {
                for line in &view.code {
                    ui.label(line);
                }
                if ui.button("new code").clicked() {
                    let _ = self.tx.send(Cmd::NewCode);
                }
            }
        });
    }
}

impl App {
    /// Edits land in the live store, so the running listener sees them immediately.
    fn forget(&mut self, id: &str) {
        let st = self.state.clone();
        let view = self.view.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let mut store = st.trust.blocking_lock();
            let note = if store.remove(&id) {
                store.save().map(|_| format!("forgot {id}")).unwrap_or_else(|e| e)
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
