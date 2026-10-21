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
│  Status  Devices  Invitations  Code            Scan LAN  Refresh │
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
