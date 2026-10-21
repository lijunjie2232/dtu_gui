[![日本語](https://img.shields.io/badge/lang-日本語-1c7ed6?style=flat-square)](README.md)
[![English](https://img.shields.io/badge/lang-English-868e96?style=flat-square)](README.en.md)
[![繁體中文](https://img.shields.io/badge/lang-繁體中文-868e96?style=flat-square)](README.zh-TW.md)

# dtu_gui

LAN 内のファイル転送を**ネイティブウィンドウ**（egui、WebView 不使用）で操作するアプリです。
`dtu_core` を自分のプロセスにホストするので、起動した時点で受信側として待ち受けます。ウィンドウを閉じればサービスも止まります。

[ドキュメント](docs/index.md) · [dtu_core](../dtu_core) · [dtu_cli](../dtu_cli) · [dtu_tui](../dtu_tui)

## ビルドと起動

```bash
cargo build --release
./target/release/dtu-gui
```

## 画面

上部にタブと操作（Scan LAN / Refresh、承認待ちがあれば件数のバッジ）、
下部に常時表示のステータスバー（短いデバイス ID・待ち受けポート・API ポート・直近の操作結果）。

| タブ | 内容 |
| --- | --- |
| Status | このデバイスの情報と、第三者向け REST の URL・エンドポイント・`copy key` |
| Devices | 信頼済みデバイスの表。行を選ぶと下のツールバーで別名の変更／信頼の取り消し |
| Invitations | 招待ごとのカード。`accept 30d` / `deny` |
| Code | 大きなペアリングコードと `new code` / `copy`、パスワードや使用済みの表示 |

## 使い方

1. 両方の端末で `dtu-gui` を起動する
2. 片方で **Scan LAN** を押して相手が見えるか確認する
3. 送る側で `dtu device add <host:port> --alias nas`（CLI）を実行する
4. 受け取る側の **Invitations** タブに招待が出るので **accept 30d** を押す
5. 以降は `dtu push nas ./file` で送れる

送る／受け取るの待ち受けはウィンドウが開いている間ずっと有効です。

## 注意

- 1 つのデータディレクトリ（`DTU_HOME`、既定 `%APPDATA%\dtu`）は 1 アプリ専有です。
  GUI を開いたまま `dtu status` を実行すると `another dtu daemon already owns …` になります。
- 転送の開始は GUI からは行いません（`dtu push` / `pull` / `async` を使います）。
- 表示は 1 秒ごとに更新されます。変更（別名・信頼の取り消し・招待の承認）は即座に反映されます。

## ドキュメント

`docs/` は MkDocs（Material + static-i18n）で、日本語 / English / 繁體中文。
`.github/workflows/docs.yml` が `main` への push で GitHub Pages に公開します。

```bash
pip install -r requirements-docs.txt
mkdocs serve
```
