[![日本語](https://img.shields.io/badge/lang-日本語-868e96?style=flat-square)](README.md)
[![English](https://img.shields.io/badge/lang-English-1c7ed6?style=flat-square)](README.en.md)
[![繁體中文](https://img.shields.io/badge/lang-繁體中文-868e96?style=flat-square)](README.zh-TW.md)

# dtu_gui

A **native window** (egui — no WebView) for file transfer on a LAN.
It hosts `dtu_core` in its own process, so it is a receiver from the moment it starts. Close the window and the services stop.

[Docs](docs/index.en.md) · [dtu_core](../dtu_core) · [dtu_cli](../dtu_cli) · [dtu_tui](../dtu_tui)

## Build and run

```bash
cargo build --release
./target/release/dtu-gui
```

## Layout

A tab bar and actions on top (Scan LAN / Refresh, plus a badge when invitations are waiting),
a permanent status bar at the bottom (short device id · listening port · API port · last result).

| Tab | Content |
| --- | --- |
| Status | this device, plus the REST URL, endpoint list and `copy key` for third-party frontends |
| Devices | table of trusted devices; select a row to rename or forget it in the toolbar |
| Invitations | one card per invitation with `accept 30d` / `deny` |
| Code | the pairing code in large type with `new code` / `copy`, password and consumed state |

## Typical flow

1. Start `dtu-gui` on both machines
2. Press **Scan LAN** on one side to see whether the peer answers
3. On the sending side run `dtu device add <host:port> --alias nas` (CLI)
4. The invitation shows up under **Invitations** on the receiver — press **accept 30d**
5. From then on `dtu push nas ./file` works

Receiving stays active for as long as the window is open.

## Notes

- One process owns the data directory (`DTU_HOME`, default `%APPDATA%\dtu`).
  Running `dtu status` while the GUI is open fails with `another dtu daemon already owns …`.
- Transfers are not started from the GUI — use `dtu push` / `pull` / `async`.
- The view refreshes every second; edits (alias, forget, accept/deny) apply immediately.

## Docs

`docs/` is MkDocs (Material + static-i18n) in Japanese / English / 繁體中文.
`.github/workflows/docs.yml` publishes it to GitHub Pages on every push to `main`.

```bash
pip install -r requirements-docs.txt
mkdocs serve
```
