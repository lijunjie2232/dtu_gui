# dtu_gui

LAN 内のファイル転送を**ネイティブウィンドウ**（egui、WebView 不使用）で操作するアプリです。
起動すると `dtu_core` を自分のプロセスにホストし、そのまま受信側として待ち受けます。ウィンドウを閉じればサービスも止まります。

## 起動

```bash
cargo build --release
./target/release/dtu-gui
```

## 画面の構成

```
┌ dtu ────────────────────────────────────────────────────────────┐
│  Status  Devices  Invitations  Code  Log  Settings      Scan LAN  Refresh │
├─────────────────────────────────────────────────────────────────┤
│ この端末の情報             │ 第三者向け REST                      │
│ device  0tZr635ykNqC3…     │ 127.0.0.1:47001        [copy key]   │
│ version 0.1.0 (proto v1)   │ GET /api/status                     │
│ listen  0.0.0.0:47000      │ GET /api/devices                    │
│ trusted 2   pending 0      │ POST /api/transfer                  │
├─────────────────────────────────────────────────────────────────┤
│ 0tZr635ykNqC…  ·  listen 47000  ·  api 127.0.0.1:47001   ready   │
└─────────────────────────────────────────────────────────────────┘
```

- 上部: タブ、右側に **Scan LAN** と **Refresh**。承認待ちの招待があれば件数のバッジ
- 下部: 常時表示のステータスバー。直近の操作結果（成功・失敗の理由）もここに出ます
- 表示は 1 秒ごとに更新

## 各タブ

### Status

このデバイスの情報（ID、バージョン、待ち受けポート、ルート、無制限モード、信頼数、承認待ち、稼働時間）と、
第三者向け REST の URL・`copy key`・主なエンドポイントの一覧。

### Devices

| 列 | 内容 |
| --- | --- |
| device | デバイス ID（等幅）。クリックで選択 |
| alias | 付けた別名（なければ `—`） |
| address | 最後に分かったアドレス |
| expires | 有効期限。期限切れは琥珀色で `expired`、無期限は `never` |
| forget | その場で信頼を取り消す |

行を選ぶと下のツールバーが使えます。別名を入力して **save**（Enter でも可）、**forget**、**clear** で選択解除。

### Invitations

招待 1 件につき 1 枚のカード。デバイス ID、相手、アドレス、相手が希望した有効期限を表示し、
**accept 30d** で承認（30 日）、**deny** で拒否します。承認するとその場で信頼リストに入ります。

### Code

大きな等幅フォントのワンタイムコード。**new code** で再生成、**copy** でクリップボードへ。
`used by one transfer` は使用済み、`a password is required` は追加パスワードが必要という意味です。

### Log

接続の認証と、実際に動いたファイルを 1 行ずつ表示します。左から時刻 / イベント / 結果 / 相手（短縮 ID・IP:port） / 詳細（パス・サイズ・操作）。
上部のセレクタで **All / Auth / Files / Invites** に絞り込めます。

この画面は「誰が何を動かしたか」を見るためのもので、転送済みファイルの一覧や管理はしません（それは OS のファイルマネージャの仕事です）。

### Settings

`config.json` の全項目をフォームで編集します。1 行は `key / 値 / 反映時期 / 説明`。
**save** で保存（クリックでも保存）、**Reload** でファイルから読み直し。
`live` の行（`code_password`）は保存と同時に動いているリスナーへ反映、`next start` の行（ポート・ルート・タイムアウト類・UI 更新間隔など）は次回起動からです。

| キー | 既定 | 内容 |
| --- | --- | --- |
| `port` / `api_port` | 47000 / 0 | 待ち受けポートと REST の loopback ポート |
| `root` | `<データディレクトリ>/share` | 相手に見せるディレクトリ |
| `parallel` | 4 | 1 ファイルあたりの TCP 本数 |
| `unlimit` / `unlimit_store_trust` | false | 危険：全接続を受け入れる |
| `code_one_shot` / `code_rotate_on_connect` | true / false | コードの使い切り・毎回ローテーション |
| `code_password` | （なし） | コードに追加するパスワード（空で解除） |
| `invite_timeout` / `session_ttl` / `idle_timeout` | 600 / 600 / 600 | 招待・セッション・無通信の時間（秒） |
| `max_conn_per_ip` / `drain_timeout` | 32 / 5 | 接続上限と停止時の猶予 |
| `ui_refresh_secs` / `ui_log_lines` | 0 / 0 | UI の更新間隔と呼び出すログ行数（0 = クライアント既定） |
| `ui_close_to_tray` | true | ウィンドウを閉じたときトレイに残す（false で完全終了） |

## トレイ

ウィンドウを閉じてもアプリは終了せず、**トレイに残って受信を続けます**（既定）。
コアのリスナー・探索・REST もそのまま動き続けます。

| 操作 | 動作 |
| --- | --- |
| トレイを左クリック | メインウィンドウを表示 |
| トレイを右クリック | メニュー：**Show dtu** / **Settings**（設定タブを開く） / **Exit**（アプリ終了） |
| ウィンドウの× | `ui_close_to_tray` が true ならトレイに隠す、false なら終了 |

- **Exit** を選ぶとコアも止まり、プロセスが終了します。
- トレイアイコンが使えない環境（一部の Linux など）では、閉じる＝終了にフォールバックします。
- 「閉じたら終了したい」場合は設定タブの `ui_close_to_tray` を false にしてください（即時反映）。

## 使い方

1. 両方の端末で `dtu-gui` を起動
2. 片方で **Scan LAN** を押し、相手が見えるか確認
3. 送る側で `dtu device add <host:port> --alias nas`（CLI）
4. 受け取る側の **Invitations** に招待が出るので **accept 30d**
5. 以降は `dtu push nas ./file`

## 注意

- 1 つのデータディレクトリ（`DTU_HOME`、既定 `%APPDATA%\dtu`）は 1 アプリ専有。
  GUI を開いたまま `dtu status` を実行すると `another dtu daemon already owns …`。
- 転送の開始は GUI の仕事ではありません（`dtu push` / `pull` / `async`）。GUI は受け取りと信頼管理を担当します。
- 別名の変更・信頼の取り消し・招待の承認は、動いているリスナーに即座に反映されます。

## このサイトについて

MkDocs（Material + static-i18n）で生成し、`main` への push で GitHub Pages に公開します。
初回だけ **Settings → Pages → Source** を **GitHub Actions** に設定してください。

```bash
pip install -r requirements-docs.txt
mkdocs serve
```

