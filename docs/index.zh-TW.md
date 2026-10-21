# dtu_gui

用**原生視窗**（egui，不含 WebView）操作 LAN 檔案傳輸的應用。
啟動時把 `dtu_core` 載入自己的行程，立刻就是收檔端；關掉視窗，服務也一起停。

## 啟動

```bash
cargo build --release
./target/release/dtu-gui
```

## 版面

```
┌ dtu ────────────────────────────────────────────────────────────┐
│  Status  Devices  Invitations  Code            Scan LAN  Refresh │
├─────────────────────────────────────────────────────────────────┤
│ 本機資訊                    │ 給第三方的 REST                     │
│ device  0tZr635ykNqC3…     │ 127.0.0.1:47001        [copy key]   │
│ version 0.1.0 (proto v1)   │ GET /api/status                     │
│ listen  0.0.0.0:47000      │ GET /api/devices                    │
│ trusted 2   pending 0      │ POST /api/transfer                  │
├─────────────────────────────────────────────────────────────────┤
│ 0tZr635ykNqC…  ·  listen 47000  ·  api 127.0.0.1:47001   ready   │
└─────────────────────────────────────────────────────────────────┘
```

- 上方：分頁，右側是 **Scan LAN** 與 **Refresh**；有邀請時顯示數量標記
- 下方：常駐狀態列，也顯示最近一次操作的結果（或失敗原因）
- 每秒更新一次

## 各分頁

### Status

本機資訊（ID、版本、監聽埠、根目錄、無限制模式、信任數、待處理數、運行時間），
以及第三方 REST 的 URL、`copy key` 與主要端點清單。

### Devices

| 欄位 | 內容 |
| --- | --- |
| device | 裝置 ID（等寬字），點擊選取 |
| alias | 你給的別名（沒有就是 `—`） |
| address | 最後已知位址 |
| expires | 到期時間；過期以琥珀色顯示 `expired`，永久則是 `never` |
| forget | 直接撤銷信任 |

選取一列後，下方工具列可用：輸入別名按 **save**（Enter 亦可）、**forget**、**clear** 取消選取。

### Invitations

每筆邀請一張卡片：裝置 ID、來源、位址、對方希望的到期時間。
**accept 30d** 代表信任 30 天，**deny** 拒絕；批准後立刻進入信任清單。

### Code

大字等寬的一次性配對碼。**new code** 重新產生，**copy** 複製到剪貼簿。
`used by one transfer` 表示已被使用，`a password is required` 表示另外設了密碼。

## 使用流程

1. 兩台機器都啟動 `dtu-gui`
2. 在其中一邊按 **Scan LAN**，確認看得到對方
3. 送檔方執行 `dtu device add <host:port> --alias nas`（CLI）
4. 收檔方的 **Invitations** 出現邀請，按 **accept 30d**
5. 之後 `dtu push nas ./file` 就能用

## 注意

- 一個資料目錄（`DTU_HOME`，預設 `%APPDATA%\dtu`）只能有一個應用。
  GUI 開著時執行 `dtu status` 會得到 `another dtu daemon already owns …`。
- 發起傳輸不是 GUI 的工作（`dtu push` / `pull` / `async`）；GUI 負責接收與信任管理。
- 改別名、撤銷信任、批准/拒絕邀請都會立刻反映到正在運行的監聽端。

## 關於本網站

用 MkDocs（Material 主題 + static-i18n）產生，push 到 `main` 時自動發佈到 GitHub Pages。
第一次請把 **Settings → Pages → Source** 設成 **GitHub Actions**。

```bash
pip install -r requirements-docs.txt
mkdocs serve
```
