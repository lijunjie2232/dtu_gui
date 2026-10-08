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

