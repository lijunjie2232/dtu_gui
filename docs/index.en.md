# dtu_gui

A **native window** (egui — no WebView) for file transfer on a LAN.
On start it hosts `dtu_core` in its own process and becomes a receiver. Close the window and the services stop.

## Run

```bash
cargo build --release
./target/release/dtu-gui
```

## Layout

```
┌ dtu ────────────────────────────────────────────────────────────┐
│  Status  Devices  Invitations  Code  Log  Settings      Scan LAN  Refresh │
├─────────────────────────────────────────────────────────────────┤
│ This device                │ Third-party API                     │
│ device  0tZr635ykNqC3…     │ 127.0.0.1:47001        [copy key]   │
│ version 0.1.0 (proto v1)   │ GET /api/status                     │
│ listen  0.0.0.0:47000      │ GET /api/devices                    │
│ trusted 2   pending 0      │ POST /api/transfer                  │
├─────────────────────────────────────────────────────────────────┤
│ 0tZr635ykNqC…  ·  listen 47000  ·  api 127.0.0.1:47001   ready   │
└─────────────────────────────────────────────────────────────────┘
```

- Top: tabs, with **Scan LAN** and **Refresh** on the right, plus a badge when invitations wait
- Bottom: a permanent status bar that also shows the last result (or the failure reason)
- The view refreshes every second

## Tabs

### Status

This device (id, version, listening port, root, unlimited mode, trust counters, uptime) plus the
REST URL, `copy key` and the main endpoints for third-party frontends.

### Devices

| Column | Content |
| --- | --- |
| device | device id in monospace — click to select |
| alias | the name you gave it (`—` when none) |
| address | last known address |
| expires | expiry; expired rows show `expired` in amber, permanent ones `never` |
| forget | revoke trust right there |

Selecting a row enables the toolbar below: type an alias and press **save** (or Enter), **forget**, or **clear** to deselect.

### Invitations

One card per invitation: device id, who, address and the expiry the peer asked for.
**accept 30d** trusts it for 30 days, **deny** refuses. Accepting adds it to the device list immediately.

### Code

The one-time code in large monospace type. **new code** rotates it, **copy** puts it on the clipboard.
`used by one transfer` means it was consumed; `a password is required` means an extra password is set.

### Log

One row per event: the connection auth, and every file that actually moved — time / event / result / peer (short id · IP:port) / detail (path, size, operation).
The selector on top filters to **All / Auth / Files / Invites**.

This view answers "who moved what"; it is not a file browser or manager.

### Settings

Every key of `config.json` as a form: `key / value / when / help`. **save** writes it (a click is
enough), **Reload** re-reads the file. `live` rows (`code_password`) reach the running listener at
once; `next start` rows (ports, root, timeouts, UI refresh…) are read when the app boots.

| Key | Default | Meaning |
| --- | --- | --- |
| `port` / `api_port` | 47000 / 0 | listener port and REST loopback port |
| `root` | `<data dir>/share` | directory shown to peers |
| `parallel` | 4 | TCP streams per file |
| `unlimit` / `unlimit_store_trust` | false | DANGEROUS: accept any device |
| `code_one_shot` / `code_rotate_on_connect` | true / false | one-shot code / rotate per connection |
| `code_password` | none | extra password on the code (empty clears) |
| `invite_timeout` / `session_ttl` / `idle_timeout` | 600 / 600 / 600 | invitation, session and silence limits (s) |
| `max_conn_per_ip` / `drain_timeout` | 32 / 5 | per-address cap and stop grace |
| `ui_refresh_secs` / `ui_log_lines` | 0 / 0 | UI refresh interval and log depth (0 = client default) |
| `ui_close_to_tray` | true | closing the window parks it in the tray (false = quit) |

## Tray

Closing the window does not quit: by default the app **stays in the tray and keeps receiving**.
The listener, discovery and REST surface keep running while it is parked.

| Action | What happens |
| --- | --- |
| left-click the tray icon | show the main window |
| right-click the tray icon | menu: **Show dtu** / **Settings** (opens the Settings tab) / **Exit** (quit the app) |
| the window's × | hide to the tray when `ui_close_to_tray` is true, quit when false |

- **Exit** stops the core with the app.
- Where no tray is available (some Linux sessions), closing falls back to quitting.
- To always quit on close, set `ui_close_to_tray` to false in the Settings tab (applies at once).

## Typical flow

1. Start `dtu-gui` on both machines
2. Press **Scan LAN** on one side to see whether the peer answers
3. On the sending side run `dtu device add <host:port> --alias nas` (CLI)
4. The invitation shows up under **Invitations** on the receiver — press **accept 30d**
5. From then on `dtu push nas ./file` works

## Notes

- One process owns the data directory (`DTU_HOME`, default `%APPDATA%\dtu`).
  Running `dtu status` while the GUI is open fails with `another dtu daemon already owns …`.
- Starting transfers is not the GUI's job (`dtu push` / `pull` / `async`); the GUI handles receiving and trust.
- Alias changes, revocation and accept/deny apply to the running listener at once.

## About this site

Built with MkDocs (Material theme + static-i18n) and published to GitHub Pages on every push to `main`.
Set **Settings → Pages → Source** to **GitHub Actions** once.

```bash
pip install -r requirements-docs.txt
mkdocs serve
```

