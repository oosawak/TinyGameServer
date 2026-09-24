# Codex Handoff

このリポジトリをGitHubへpushした後、サーバー側Codexではこの文書を最初に読んでください。

## Project

GameForge Server

Rust製のゲーム専用サーバー基盤。

## Core decisions — 勝手に変更しない

1. Rustをサーバー実装の中心にする。
2. BrowserとTauriの両方をクライアント対象にする。
3. v0.1の外部通信はHTTP + WebSocket。
4. 将来WebTransport / QUICを追加する。
5. 標準永続DBはSQLite。
6. PostgreSQLを必須依存にしない。
7. 基本分離単位は `1 User Group = 1 Rust Process = 1 SQLite`。
8. Group Processはlocalhostのみで待ち受ける。
9. 外部公開窓口はGateway。
10. SupervisorがGroup Processをオンデマンド起動する。
11. realtime stateを毎tick SQLiteへ保存しない。
12. 将来WASM Game Cartridgeを追加できる構造を保つ。

## Current crates

```text
gameforge-common
gameforge-gateway
gameforge-supervisor
gameforge-group-server
```

## First task on Codex server

まずコードを変更せず以下を実施すること。

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
```

問題があれば、設計を大きく変更せずコンパイルエラーを修正する。

その後:

1. `cargo build --workspace`
2. Supervisor起動
3. Gateway起動
4. `/health`確認
5. `demo` groupへPUT
6. SQLite生成確認
7. GETで永続値確認
8. browser sampleからWebSocket接続
9. 2 client間broadcast確認
10. idle timeout後のprocess終了確認
11. 再アクセス時の再spawn確認
12. 保存データが残っていることを確認

## Do not do yet

初版の動作確認前に以下を追加しないこと。

- Kubernetes
- PostgreSQL必須化
- Redis必須化
- microserviceの追加分割
- Docker依存化
- WebTransportへの全面置換
- WASM runtime

まずv0.1を小さく完成させる。

## After v0.1 validation

優先順位:

1. graceful group stop API
2. SQLite backup API
3. authentication/group authorization
4. transport abstraction
5. WebTransport / HTTP3 / QUIC prototype
6. binary packet format
7. WASM cartridge design
8. multi-host routing
