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
│  Status  Devices  Invitations  Code            Scan LAN  Refresh │
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
