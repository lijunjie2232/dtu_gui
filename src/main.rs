//! dtu desktop client: a native window (egui). It *is* the app: the core listener,
//! discovery and REST surface start with it and stop when it exits.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dtu_core::proto::{self, InviteView};
use dtu_core::state::{Decision, State};
use dtu_core::trust::Store;
use dtu_core::{cfg, discover, embed, guard, identity, invite};
use eframe::egui;

const TABS: [&str; 4] = ["Status", "Devices", "Invitations", "Pairing code"];

#[derive(Default, Clone)]
struct View {
    status: Vec<(String, String)>,
    devices: Vec<String>,
    invites: Vec<InviteView>,
    code: Vec<String>,
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
    let mut v = View {
        status: vec![
            ("device".into(), st.id.id.clone()),
            ("version".into(), format!("{} (proto v{})", dtu_core::VERSION, proto::PROTO)),
            ("port".into(), st.cfg.port.to_string()),
            ("api".into(), format!("127.0.0.1:{}", dtu_core::api::api_port(st))),
            ("root".into(), guard::for_display(&st.root)),
            ("unlimit".into(), st.cfg.unlimit.to_string()),
            ("trusted".into(), st.trust.lock().await.devices.len().to_string()),
            ("pending".into(), st.invites.lock().await.len().to_string()),
            ("uptime".into(), format!("{}s", identity::now().saturating_sub(st.started))),
        ],
        ..Default::default()
    };

    let store: Store = st.trust.lock().await.clone();
    for (id, d) in &store.devices {
        let expires = if d.expires_at == 0 {
            "never".to_string()
        } else {
            dtu_core::audit::iso(d.expires_at)
        };
        let live = if store.get(id).is_some() { "" } else { " (expired)" };
        v.devices.push(format!(
            "{id}  {}  {}  expires {expires}{live}",
            d.alias.as_deref().unwrap_or("-"),
            d.addr.clone().unwrap_or_else(|| "-".into())
        ));
    }

    v.invites = invite::list(st).await;
    let c = st.code.lock().await;
    v.code = vec![
        format!("code  {}", c.code),
        format!("password  {}", if c.password.is_some() { "required" } else { "not set" }),
        format!("state  {}", if c.consumed { "consumed (refresh to reuse)" } else { "fresh" }),
    ];
    v
}

async fn act(st: &Arc<State>, cmd: Cmd) -> Result<String, String> {
    match cmd {
        Cmd::Refresh => Ok(String::new()),
        Cmd::Scan => {
            let trust = st.trust.lock().await.clone();
            let items = discover::scan(st.cfg.port, 1500, &st.id.id, &trust).await;
            let found: Vec<String> = items
                .iter()
                .map(|s| {
                    format!(
                        "{} {}{}",
                        s.id,
                        s.addr,
                        if s.trusted { " trusted" } else { "" }
                    )
                })
                .collect();
            Ok(if found.is_empty() {
                "no device answered".to_string()
            } else {
                found.join(", ")
            })
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
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
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
