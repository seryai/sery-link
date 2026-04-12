# Backend API Specification for Desktop Agent

These endpoints need to be implemented in your FastAPI backend (`backend/app/api/v1/`) to support the desktop agent.

## 1. Agent Authorization Flow

### Endpoint: `GET /agent/authorize`

**Purpose:** Show authorization page where user approves the desktop agent.

**Query Parameters:**
- `agent_name` (string) - User-provided name (e.g., "MacBook Pro")
- `platform` (string) - OS platform ("macOS", "Windows", "Linux")
- `hostname` (string) - Machine hostname
- `redirect_uri` (string) - OAuth callback URL (http://localhost:7777)

**Response:** HTML page with authorization form

**Example Request:**
```
GET /agent/authorize?agent_name=MacBook+Pro&platform=macOS&hostname=Johns-MacBook.local&redirect_uri=http://localhost:7777
```

**HTML Response:**
```html
<!DOCTYPE html>
<html>
<head>
    <title>Authorize Desktop Agent</title>
</head>
<body>
    <h1>Authorize "MacBook Pro"?</h1>
    <p>Platform: macOS</p>
    <p>Hostname: Johns-MacBook.local</p>

    <form method="POST" action="/agent/approve">
        <input type="hidden" name="agent_name" value="MacBook Pro">
        <input type="hidden" name="platform" value="macOS">
        <input type="hidden" name="hostname" value="Johns-MacBook.local">
        <input type="hidden" name="redirect_uri" value="http://localhost:7777">
        <input type="hidden" name="user_id" value="{{ current_user.id }}">
        <button type="submit">Approve</button>
        <button type="button" onclick="window.close()">Deny</button>
    </form>
</body>
</html>
```

**Backend Logic:**
1. Verify user is authenticated (session/cookie)
2. Generate authorization code
3. Store code temporarily (Redis with 5-minute TTL)
4. On form submit, redirect to: `{redirect_uri}?code={authorization_code}`

---

## 2. Token Exchange

### Endpoint: `POST /api/v1/agent/token`

**Purpose:** Exchange authorization code for access token.

**Request Body:**
```json
{
  "code": "abc123def456"
}
```

**Response (Success - 200):**
```json
{
  "access_token": "eyJhbGc...",
  "agent_id": "agent_abc123",
  "workspace_id": "ws_xyz789",
  "expires_in": 31536000
}
```

**Response (Error - 401):**
```json
{
  "detail": "Invalid or expired authorization code"
}
```

**Backend Logic:**
1. Validate authorization code from Redis
2. Create new `Agent` record in database:
   ```python
   agent = Agent(
       id=generate_agent_id(),
       workspace_id=user.workspace_id,
       name=stored_agent_name,
       platform=stored_platform,
       hostname=stored_hostname,
       status="online",
       last_ping=datetime.utcnow()
   )
   ```
3. Generate JWT access token with claims:
   ```python
   {
       "sub": agent.id,
       "workspace_id": agent.workspace_id,
       "type": "agent",
       "exp": now + timedelta(days=365)
   }
   ```
4. Delete authorization code from Redis
5. Return token + agent info

**Database Schema:**
```sql
CREATE TABLE agents (
    id VARCHAR(255) PRIMARY KEY,
    workspace_id VARCHAR(255) REFERENCES workspaces(id),
    name VARCHAR(255),
    platform VARCHAR(50),
    hostname VARCHAR(255),
    status VARCHAR(20),  -- 'online', 'offline'
    last_ping TIMESTAMP,
    created_at TIMESTAMP,
    updated_at TIMESTAMP
);
```

---

## 3. Agent Info Verification

### Endpoint: `GET /api/v1/agent/info`

**Purpose:** Verify agent token is still valid and get agent details.

**Headers:**
- `Authorization: Bearer {access_token}`

**Response (Success - 200):**
```json
{
  "access_token": "eyJhbGc...",
  "agent_id": "agent_abc123",
  "workspace_id": "ws_xyz789",
  "expires_in": 31535000
}
```

**Response (Error - 401):**
```json
{
  "detail": "Invalid or expired token"
}
```

**Backend Logic:**
1. Verify JWT token
2. Extract agent_id from token claims
3. Query agent from database
4. Update `last_ping` timestamp
5. Return agent info

---

## 4. Metadata Sync

### Endpoint: `POST /api/v1/agent/sync-metadata`

**Purpose:** Sync dataset metadata from desktop agent to cloud.

**Headers:**
- `Authorization: Bearer {access_token}`

**Request Body:**
```json
{
  "datasets": [
    {
      "relative_path": "sales/2024/january.parquet",
      "file_format": "parquet",
      "size_bytes": 1048576,
      "row_count_estimate": 10000,
      "schema": [
        {
          "name": "order_id",
          "type": "Int64",
          "nullable": false
        },
        {
          "name": "amount",
          "type": "Float64",
          "nullable": true
        }
      ],
      "last_modified": "2024-01-15T10:30:00Z"
    }
  ]
}
```

**Response (Success - 200):**
```json
{
  "total_synced": 1,
  "datasets": [
    {
      "id": "ds_abc123",
      "relative_path": "sales/2024/january.parquet",
      "status": "synced"
    }
  ]
}
```

**Backend Logic:**
1. Verify JWT token and extract agent_id
2. For each dataset:
   - Check if dataset already exists (by agent_id + relative_path)
   - If exists: UPDATE metadata
   - If new: INSERT dataset record
3. Store in `datasets` table:
   ```python
   dataset = Dataset(
       id=generate_dataset_id(),
       agent_id=agent_id,
       workspace_id=workspace_id,
       name=extract_name_from_path(relative_path),
       relative_path=relative_path,
       file_format=file_format,
       size_bytes=size_bytes,
       row_count_estimate=row_count_estimate,
       schema=json.dumps(schema),  # Store as JSON
       last_modified=last_modified,
       last_synced=datetime.utcnow()
   )
   ```
4. Generate embeddings for semantic search (async task):
   ```python
   description = f"Dataset: {name}, Format: {file_format}, Columns: {', '.join(col['name'] for col in schema)}"
   embedding = await generate_embedding(description)
   dataset.embedding = embedding
   ```

**Database Schema:**
```sql
CREATE TABLE datasets (
    id VARCHAR(255) PRIMARY KEY,
    agent_id VARCHAR(255) REFERENCES agents(id),
    workspace_id VARCHAR(255) REFERENCES workspaces(id),
    name VARCHAR(255),
    relative_path TEXT,
    file_format VARCHAR(50),
    size_bytes BIGINT,
    row_count_estimate BIGINT,
    schema JSONB,
    embedding VECTOR(1024),  -- pgvector
    last_modified TIMESTAMP,
    last_synced TIMESTAMP,
    created_at TIMESTAMP,
    updated_at TIMESTAMP
);

CREATE INDEX idx_datasets_agent ON datasets(agent_id);
CREATE INDEX idx_datasets_workspace ON datasets(workspace_id);
CREATE INDEX idx_datasets_embedding ON datasets USING ivfflat (embedding vector_cosine_ops);
```

---

## 5. WebSocket Tunnel

### Endpoint: `WS /api/v1/agent/tunnel`

**Purpose:** Persistent WebSocket connection for real-time query execution.

**Headers:**
- `Authorization: Bearer {access_token}`
- `Upgrade: websocket`

**Connection Flow:**

1. **Agent Connects:**
   ```
   WS /api/v1/agent/tunnel
   Authorization: Bearer eyJhbGc...
   ```

2. **Server Validates Token:**
   - Extract agent_id from JWT
   - Store WebSocket connection in memory (agent_id → ws_connection)
   - Update agent status to "online"

3. **Heartbeat (Server → Agent):**
   ```json
   {
     "type": "ping",
     "timestamp": "2024-01-15T10:30:00Z"
   }
   ```

4. **Heartbeat Response (Agent → Server):**
   ```json
   {
     "type": "pong",
     "timestamp": "2024-01-15T10:30:00Z"
   }
   ```

5. **Query Execution (Server → Agent):**
   ```json
   {
     "type": "run_sql",
     "query_id": "q_abc123",
     "sql": "SELECT * FROM {{file}} WHERE date > '2024-01-01' LIMIT 100",
     "file_path": "/Users/john/data/sales/2024/january.parquet"
   }
   ```

6. **Query Result (Agent → Server):**
   ```json
   {
     "type": "query_result",
     "query_id": "q_abc123",
     "columns": ["order_id", "amount", "date"],
     "rows": [
       [1001, 99.99, "2024-01-15"],
       [1002, 149.99, "2024-01-16"]
     ],
     "row_count": 2,
     "execution_ms": 45
   }
   ```

7. **Query Error (Agent → Server):**
   ```json
   {
     "type": "query_error",
     "query_id": "q_abc123",
     "error": "File not found: /Users/john/data/missing.parquet",
     "suggestion": "Check if the file path is accessible"
   }
   ```

**Backend Implementation (FastAPI + WebSockets):**

```python
from fastapi import WebSocket, WebSocketDisconnect, Depends
from app.services.auth import verify_agent_token
import json
import asyncio

# Global connection manager
class AgentConnectionManager:
    def __init__(self):
        self.active_connections: dict[str, WebSocket] = {}

    async def connect(self, agent_id: str, websocket: WebSocket):
        await websocket.accept()
        self.active_connections[agent_id] = websocket

    def disconnect(self, agent_id: str):
        self.active_connections.pop(agent_id, None)

    async def send_query(self, agent_id: str, query_data: dict):
        ws = self.active_connections.get(agent_id)
        if ws:
            await ws.send_json(query_data)
            return True
        return False

manager = AgentConnectionManager()

@app.websocket("/api/v1/agent/tunnel")
async def agent_tunnel(
    websocket: WebSocket,
    token: str = Query(...),  # Or from Authorization header
):
    # Verify token
    agent_id = verify_agent_token(token)  # Returns agent_id or raises 401

    # Connect
    await manager.connect(agent_id, websocket)

    # Update agent status
    await db.execute(
        "UPDATE agents SET status = 'online', last_ping = NOW() WHERE id = :agent_id",
        {"agent_id": agent_id}
    )

    # Heartbeat task
    async def send_heartbeat():
        while True:
            try:
                await websocket.send_json({
                    "type": "ping",
                    "timestamp": datetime.utcnow().isoformat()
                })
                await asyncio.sleep(30)
            except:
                break

    heartbeat_task = asyncio.create_task(send_heartbeat())

    try:
        while True:
            # Receive messages from agent
            data = await websocket.receive_json()

            if data["type"] == "pong":
                # Update last_ping
                await db.execute(
                    "UPDATE agents SET last_ping = NOW() WHERE id = :agent_id",
                    {"agent_id": agent_id}
                )

            elif data["type"] == "query_result":
                # Store result for pending query
                await handle_query_result(data)

            elif data["type"] == "query_error":
                # Store error for pending query
                await handle_query_error(data)

    except WebSocketDisconnect:
        manager.disconnect(agent_id)
        await db.execute(
            "UPDATE agents SET status = 'offline' WHERE id = :agent_id",
            {"agent_id": agent_id}
        )
        heartbeat_task.cancel()

# Function to send query from AI agent
async def execute_agent_query(agent_id: str, sql: str, file_path: str):
    query_id = generate_query_id()

    success = await manager.send_query(agent_id, {
        "type": "run_sql",
        "query_id": query_id,
        "sql": sql,
        "file_path": file_path
    })

    if not success:
        raise Exception("Agent not connected")

    # Wait for result (with timeout)
    result = await wait_for_query_result(query_id, timeout=30)
    return result
```

---

## Implementation Order

1. ✅ **Start with Token Exchange** - Simplest, allows testing auth flow
2. ✅ **Add Agent Info Endpoint** - Verify tokens work
3. ✅ **Implement Authorization Page** - Complete OAuth flow
4. ✅ **Add Metadata Sync** - Enable folder watching
5. ✅ **Implement WebSocket Tunnel** - Enable query execution

---

## Testing the Flow

### 1. Test Token Exchange (Without UI)
```bash
# Manually create auth code in Redis
redis-cli SET "auth_code:test123" '{"user_id":"user_1","agent_name":"Test Agent","platform":"macOS","hostname":"test.local"}' EX 300

# Exchange for token
curl -X POST http://localhost:8000/api/v1/agent/token \
  -H "Content-Type: application/json" \
  -d '{"code":"test123"}'

# Should return:
# {
#   "access_token": "eyJhbGc...",
#   "agent_id": "agent_abc123",
#   "workspace_id": "ws_xyz789",
#   "expires_in": 31536000
# }
```

### 2. Test Agent Info
```bash
curl http://localhost:8000/api/v1/agent/info \
  -H "Authorization: Bearer eyJhbGc..."
```

### 3. Test Metadata Sync
```bash
curl -X POST http://localhost:8000/api/v1/agent/sync-metadata \
  -H "Authorization: Bearer eyJhbGc..." \
  -H "Content-Type: application/json" \
  -d '{
    "datasets": [{
      "relative_path": "test.parquet",
      "file_format": "parquet",
      "size_bytes": 1024,
      "row_count_estimate": 100,
      "schema": [{"name": "id", "type": "Int64", "nullable": false}],
      "last_modified": "2024-01-15T10:30:00Z"
    }]
  }'
```

### 4. Test WebSocket
```javascript
// Use wscat or browser console
const ws = new WebSocket('ws://localhost:8000/api/v1/agent/tunnel?token=eyJhbGc...');

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log('Received:', data);

  if (data.type === 'ping') {
    ws.send(JSON.stringify({
      type: 'pong',
      timestamp: data.timestamp
    }));
  }
};
```

---

## Security Considerations

1. **Token Expiry**: Set reasonable expiry (1 year for agents)
2. **Rate Limiting**: Limit token exchange attempts
3. **Code Expiry**: Authorization codes expire in 5 minutes
4. **Workspace Isolation**: Always filter by workspace_id
5. **HTTPS Only**: Enforce HTTPS in production
6. **CORS**: Configure for agent authorization page

---

Once these endpoints are implemented, the desktop agent will be **fully functional**! 🎉
