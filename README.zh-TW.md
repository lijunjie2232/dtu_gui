[![日本語](https://img.shields.io/badge/lang-日本語-868e96?style=flat-square)](README.md)
[![English](https://img.shields.io/badge/lang-English-868e96?style=flat-square)](README.en.md)
[![繁體中文](https://img.shields.io/badge/lang-繁體中文-1c7ed6?style=flat-square)](README.zh-TW.md)

# dtu_gui

用**原生視窗**（egui，不含 WebView）操作 LAN 檔案傳輸的應用。
它把 `dtu_core` 載入自己的行程，所以一啟動就是收檔端；關掉視窗，服務也一起停。

[文件](docs/index.zh-TW.md) · [dtu_core](../dtu_core) · [dtu_cli](../dtu_cli) · [dtu_tui](../dtu_tui)

## 建置與啟動

```bash
cargo build --release
./target/release/dtu-gui
```

## 版面

上方是分頁與動作（Scan LAN / Refresh，有邀請時顯示數量標記），
下方是常駐狀態列（短裝置 ID · 監聽埠 · API 埠 · 最近一次結果）。

| 分頁 | 內容 |
| --- | --- |
| Status | 本機資訊，以及給第三方的 REST URL、端點清單與 `copy key` |
| Devices | 已信任裝置表格；選一列後可在下方工具列改別名或取消信任 |
| Invitations | 每筆邀請一張卡片，附 `accept 30d` / `deny` |
| Code | 大字配對碼與 `new code` / `copy`，並顯示是否需要密碼、是否已用 |

## 使用流程

1. 兩台機器都啟動 `dtu-gui`
2. 在其中一邊按 **Scan LAN**，確認看得到對方
3. 送檔方執行 `dtu device add <host:port> --alias nas`（CLI）
4. 收檔方的 **Invitations** 分頁出現邀請，按 **accept 30d**
5. 之後 `dtu push nas ./file` 就能用

視窗開著期間，收檔的監聽一直有效。

## 注意

- 一個資料目錄（`DTU_HOME`，預設 `%APPDATA%\dtu`）只能有一個應用。
  GUI 開著時執行 `dtu status` 會得到 `another dtu daemon already owns …`。
- GUI 不发起傳輸，請用 `dtu push` / `pull` / `async`。
- 畫面每秒更新一次；操作（改別名、取消信任、批准/拒絕邀請）會立即生效。

## 文件

`docs/` 用 MkDocs（Material + static-i18n），三種語言：日本語 / English / 繁體中文。
`.github/workflows/docs.yml` 會在 push 到 `main` 時發佈到 GitHub Pages。

```bash
pip install -r requirements-docs.txt
mkdocs serve
```
