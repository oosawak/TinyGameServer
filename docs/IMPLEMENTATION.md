# v0.1 Implementation Specification

## Ports

Default:

```text
Gateway    0.0.0.0:8787
Supervisor 127.0.0.1:8788
Group      dynamic localhost port
```

Group ProcessのportはSupervisorが空きportを取得して渡します。

## HTTP endpoints

### Gateway

```text
GET  /health
GET  /api/groups/{group_id}/state/{key}
PUT  /api/groups/{group_id}/state/{key}
GET  /ws/{group_id}   (WebSocket upgrade)
```

### Supervisor

```text
GET  /health
GET  /groups
POST /groups/{group_id}/ensure
```

### Group Process

```text
GET /health
GET /api/state/{key}
PUT /api/state/{key}
GET /ws   (WebSocket upgrade)
```

## Process creation

Gatewayが`demo`へ初めてアクセス:

```text
Gateway
  -> POST Supervisor /groups/demo/ensure

Supervisor
  -> free localhost port
  -> data/groups/de/demo/game.db
  -> spawn gameforge-group-server

Group Process
  -> SQLite open/create
  -> WAL mode
  -> bind localhost

Supervisor
  -> readiness check
  -> return port

Gateway
  -> proxy request
```

## Idle shutdown

Group Processは以下を保持します。

```text
last_activity
active_websocket_count
```

条件:

```text
active websocket = 0
AND
now - last_activity >= idle timeout
```

となった場合:

```text
close SQLite pool
exit process
```

WebSocketが接続中ならidle shutdownしません。

## SQLite v0.1 schema

初版では仕組みの検証を優先して最小KVのみです。

```sql
CREATE TABLE kv (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
```

これは最終ゲームDBスキーマではありません。

次の段階で、例えば以下へ分離します。

```text
players
characters
inventory
save_slots
world_state
events
```

## Known v0.1 limitations

- authenticationなし
- TLSなし
- group authorizationなし
- Supervisor自体の永続directory DBなし
- process resource limitなし
- graceful backup commandなし
- WebSocketはbinary broadcastへ統一
- binary game protocol未定
- WebTransport未実装
- WASM cartridge未実装
- Supervisor停止時の既存Group再発見未実装
- dynamic port確保とchild bindの間に小さなrace windowあり

これらはv0.1では意図した制限です。
