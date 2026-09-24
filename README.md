# GameForge Server v0.1

ブラウザ / Tauri ゲーム向けの軽量な Rust 製ゲームサーバー基盤です。

最初の設計方針は非常に単純です。

```text
1 User Group = 1 Rust Process = 1 SQLite Database
```

巨大な共有DBやKubernetesを最初から必須にせず、小規模な1台構成から始め、必要になった時だけ複数サーバーへ拡張できることを目標にしています。

## Architecture

```text
Browser / Tauri
      |
      | HTTP / WebSocket
      v
+------------------+
| Gateway :8787    |
+--------+---------+
         |
         | ensure group
         v
+------------------+
| Supervisor :8788 |
+--------+---------+
         |
         | spawn / monitor
         v
+---------------------+       +---------------------+
| Group Process A     |       | Group Process B     |
| random localhost port       | random localhost port
| group=A             |       | group=B             |
| SQLite A            |       | SQLite B            |
+---------------------+       +---------------------+
```

Group Process は外部公開せず、`127.0.0.1` のみにbindします。外部クライアントはGatewayだけに接続します。

## v0.1で入っているもの

- Cargo workspace
- `gameforge-gateway`
- `gameforge-supervisor`
- `gameforge-group-server`
- `gameforge-common`
- HTTP health check
- Group Process のオンデマンド起動
- GroupごとのSQLite自動作成
- SQLite WAL mode
- HTTP経由の簡単な永続key/value
- WebSocket中継
- 同一Group内WebSocket broadcast
- idle timeoutによるGroup Process終了
- 再アクセス時のGroup Process再起動
- ブラウザ動作確認用HTML

## Repository structure

```text
gameforge-server-v0.1/
├─ Cargo.toml
├─ README.md
├─ docs/
│  ├─ ARCHITECTURE.md
│  ├─ IMPLEMENTATION.md
│  └─ CODEX_HANDOFF.md
├─ crates/
│  ├─ common/
│  ├─ gateway/
│  ├─ supervisor/
│  └─ group-server/
├─ examples/
│  └─ browser/index.html
└─ data/
   └─ .gitkeep
```

## Build

Rust stable をインストールした環境でリポジトリ直下から:

```bash
cargo build --workspace
```

## Run

3つの実行ファイルをビルドしますが、通常手動で起動するのは Supervisor と Gateway の2つだけです。

### 1. Supervisor

Linux/macOS:

```bash
RUST_LOG=info cargo run -p gameforge-supervisor
```

PowerShell:

```powershell
$env:RUST_LOG="info"
cargo run -p gameforge-supervisor
```

### 2. Gateway

別ターミナルで:

```bash
RUST_LOG=info cargo run -p gameforge-gateway
```

PowerShell:

```powershell
$env:RUST_LOG="info"
cargo run -p gameforge-gateway
```

> Supervisor は `target/debug/gameforge-group-server` を自動起動します。そのため最初に `cargo build --workspace` を実行してください。

## Test API

Gateway health:

```bash
curl http://localhost:8787/health
```

値を保存:

```bash
curl -X PUT http://localhost:8787/api/groups/demo/state/player_name \
  -H "Content-Type: application/json" \
  -d '{"value":"Koushirou"}'
```

値を取得:

```bash
curl http://localhost:8787/api/groups/demo/state/player_name
```

初回アクセス時に `demo` Group Process が自動起動し、概ね以下にDBが作成されます。

```text
data/groups/de/demo/game.db
```

## Browser test

`examples/browser/index.html` をローカルHTTPサーバーから開いてください。

例:

```bash
python3 -m http.server 8000 --directory examples/browser
```

その後:

```text
http://localhost:8000/
```

で開きます。

複数タブを同じGroup名で接続して `Send Hello` を押すと、同じGroup Processのbroadcastを確認できます。

## Environment variables

### Gateway

```text
GAMEFORGE_GATEWAY_BIND=0.0.0.0:8787
GAMEFORGE_SUPERVISOR_URL=http://127.0.0.1:8788
```

### Supervisor

```text
GAMEFORGE_SUPERVISOR_BIND=127.0.0.1:8788
GAMEFORGE_DATA_ROOT=./data/groups
GAMEFORGE_GROUP_SERVER=/custom/path/gameforge-group-server
GAMEFORGE_GROUP_IDLE_SECONDS=300
```

## Security

v0.1 はローカル開発用プロトタイプです。まだ認証・TLS・rate limitは実装していません。

本番公開前には最低でも以下が必要です。

- HTTPS/WSS
- authentication
- authorization
- group membership validation
- rate limit
- request/body size limit
- process resource limit
- secure secret storage
- backup / restore
- audit logs

特に、クライアントが指定した `group_id` をそのまま信頼する設計のまま本番運用してはいけません。

## Next

v0.2候補:

```text
Transport abstraction
+ WebTransport
+ HTTP/3 / QUIC
+ binary protocol
+ authentication
```

その後:

```text
WASM Game Cartridge
+ game.json
+ game.wasm
+ host API
+ CPU/memory limits
```

詳細は `docs/` を参照してください。
