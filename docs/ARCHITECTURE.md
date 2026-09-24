# Architecture

## Design goal

GameForge Serverは「汎用クラウド基盤」ではなく、「ゲームを動かすための小さなサーバープラットフォーム」を目指します。

中心となる単位はUser Groupです。

```text
1 User Group
= 1 isolated Rust process
= 1 SQLite database
```

## Gateway

Gatewayだけを外部へ公開します。

役割:

- HTTP受付
- WebSocket受付
- 将来WebTransport受付
- 認証（将来）
- group_id解決
- Supervisorへの起動要求
- Group Processへの中継

ゲーム状態はGatewayに持たせません。

## Supervisor

Supervisorはlocalhost専用の管理プロセスです。

役割:

- Group Processの起動
- PID/port管理
- dead process検出
- 再起動
- data path決定
- 将来のbackup/migration/resource control

v0.1ではGroup Processがidle timeoutを自己判定して終了し、Supervisorは次回アクセス時に終了済みプロセスを検出して再起動します。

## Group Process

Group専用のauthoritative runtimeです。

役割:

- Room
- Players
- WebSocket broadcast
- transient game state
- Group SQLite
- 将来WASM game cartridge

外部から直接アクセスさせず、localhostのみで待ち受けます。

## SQLite layout

```text
data/groups/
  de/
    demo/
      game.db
  gu/
    guild-001/
      game.db
```

先頭2文字をshard directoryとして利用しています。将来大量のGroupが存在した時に1ディレクトリへファイルが集中するのを避けるためです。

## RAM vs SQLite

RAM:

```text
position
velocity
bullets
enemy AI
current frame state
temporary combat state
```

SQLite:

```text
characters
inventory
progression
save data
persistent world state
settings
```

毎tickの座標をSQLiteに保存しません。

## Future transport

ゲームコードには以下の意味だけを見せる予定です。

```rust
pub enum Delivery {
    Reliable,
    Unreliable,
}
```

v0.1:

```text
Reliable   -> WebSocket
Unreliable -> WebSocket (temporary fallback)
```

v0.2以降:

```text
Reliable   -> WebTransport Stream
Unreliable -> WebTransport Datagram
```

Tauri native clientでは将来的にnative QUIC transportも選択可能にします。

## Scaling model

最初:

```text
Host A
  Gateway
  Supervisor
  Group A
  Group B
  Group C
```

将来:

```text
Public Gateway
  |
  +-- Host A / Supervisor
  |     Group A
  |     Group B
  |
  +-- Host B / Supervisor
        Group C
        Group D
```

DBがGroup単位のファイルなので、停止→snapshot→転送→別Host起動という比較的単純なmigrationモデルに発展させられます。
