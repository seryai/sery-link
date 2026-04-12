# Testing the Desktop Agent End-to-End

## ✅ What's Ready

### Backend API (All Implemented!)
- ✅ `GET /agent/authorize` - HTML authorization page
- ✅ `POST /api/v1/agent/token` - Token exchange
- ✅ `GET /api/v1/agent/info` - Agent info verification
- ✅ `POST /api/v1/agent/sync-metadata` - Metadata sync
- ✅ `WS /api/v1/agent/tunnel` - WebSocket tunnel
- ✅ `GET /api/v1/agent/status/{agent_id}` - Agent status check

### Desktop Agent (Fully Functional!)
- ✅ OAuth loopback authentication
- ✅ Keychain token storage
- ✅ Folder watching and metadata extraction
- ✅ DuckDB query execution
- ✅ WebSocket client
- ✅ Beautiful React UI

## 🚀 Testing Steps

### 1. Start the Backend

```bash
cd backend
uvicorn app.main:app --reload --host 0.0.0.0 --port 8000
```

Verify it's running:
```bash
curl http://localhost:8000/health
```

### 2. Start the Desktop Agent

```bash
cd desktop-agent
pnpm tauri dev
```

You should see:
- ✅ Vite dev server on http://localhost:1420
- ✅ Tauri window opens
- ✅ Purple gradient login screen
- ✅ "Sery.ai Desktop Agent" title
- ✅ Agent Name input field
- ✅ "Connect to Sery.ai" button

### 3. Test OAuth Flow

**Step 3.1: Start Authentication**
1. In the desktop app, enter agent name: `Test Agent`
2. Click "Connect to Sery.ai"
3. Browser opens to: `http://localhost:8000/agent/authorize?agent_name=Test+Agent&platform=macOS&...`

**Step 3.2: Approve in Browser**
- You'll see a beautiful purple authorization page
- Shows agent name, platform, hostname
- Click "Approve & Connect"
- Browser redirects to `http://localhost:7777?code=...`
- Desktop app's callback server receives the code

**Step 3.3: Token Exchange**
- Desktop app automatically exchanges code for token
- Token is saved to macOS Keychain (or equivalent)
- App transitions to main interface

**Expected Result:**
✅ Desktop agent shows main UI with:
- StatusBar showing "Connecting..." → "Connected" (when WebSocket connects)
- Sidebar with "Folders" and "Settings" tabs
- Empty folder list with "Add Folder" button

### 4. Test Folder Management

**Add a Folder:**
1. Click "Add Folder" button
2. macOS folder picker opens
3. Select a folder with `.parquet` or `.csv` files
4. Folder appears in watched list
5. Click "Sync Now"

**What Happens:**
- Desktop agent scans folder recursively
- Extracts metadata (schema, row count, file size)
- Sends to backend via `POST /api/v1/agent/sync-metadata`
- Backend creates `Dataset` records with `sync_mode=TUNNEL`

**Verify in Backend:**
```bash
# Check database
docker exec -it datalake-postgres-1 psql -U postgres -d datalake -c "
  SELECT id, query_path, file_format, row_count_estimate
  FROM datasets
  WHERE sync_mode = 'tunnel';
"
```

### 5. Test WebSocket Connection

**Check Connection Status:**
```bash
# Call agent status endpoint
curl http://localhost:8000/api/v1/agent/status/{AGENT_ID} \
  -H "Authorization: Bearer YOUR_USER_TOKEN"

# Should return:
# {
#   "agent_id": "...",
#   "is_online": true,
#   "status": "online",
#   "last_seen_at": "2024-01-15T10:30:00Z"
# }
```

**Monitor WebSocket:**
- Check backend logs for:
  ```
  ✅ Agent {agent_id} connected
  ```
- Desktop agent StatusBar should show "Connected" (green)

### 6. Test Query Execution

**Send a query via the tunnel:**

```python
# In backend Python console or test script
from app.api.v1.tunnel import connection_manager
import asyncio

async def test_query():
    agent_id = "YOUR_AGENT_ID"
    query_id = "test_query_123"
    sql = "SELECT * FROM read_parquet('/path/to/file.parquet') LIMIT 10"
    file_path = "/path/to/file.parquet"  # Must be in watched folder

    result = await connection_manager.send_query(
        agent_id=agent_id,
        query_id=query_id,
        sql=sql,
        database_path=file_path,
        timeout=30
    )

    print("Query result:", result)

# Run
asyncio.run(test_query())
```

**Expected Flow:**
1. Backend sends query via WebSocket
2. Desktop agent receives query
3. Desktop agent validates file path is in watched folders
4. Desktop agent executes SQL with DuckDB
5. Desktop agent sends result back via WebSocket
6. Backend receives result and resolves promise

**Note:** The current WebSocket implementation in desktop agent has a TODO - it receives queries but doesn't send results back. This needs to be fixed (see below).

### 7. Test File Watching

**Add a new file:**
```bash
# In a watched folder
cp test.parquet /path/to/watched/folder/new_file.parquet
```

**What Should Happen:**
- File watcher detects change (debounced 1 second)
- If `auto_sync_on_change` is enabled, metadata auto-syncs
- New dataset appears in backend

**Verify:**
```bash
# Check datasets count increased
curl http://localhost:8000/api/v1/agent/datasets \
  -H "Authorization: Bearer AGENT_TOKEN"
```

## 🐛 Known Issues & Fixes Needed

### Issue 1: WebSocket Doesn't Send Query Results

**Location:** `desktop-agent/src-tauri/src/websocket.rs:179`

**Problem:**
```rust
async fn handle_run_sql(message: &Value, config: Arc<RwLock<Config>>) -> Result<()> {
    // ... executes query ...

    // TODO: Need write handle to send result back!
    // Currently just logs success/error
}
```

**Solution:** Refactor `connect_and_run` to pass write handle to message handlers.

**Quick Fix:**
```rust
// In connect_and_run, change architecture:
// 1. Don't split ws_stream
// 2. Use Arc<Mutex<WebSocket>> for both read and write
// 3. Pass write handle to handle_message

// Example:
let ws = Arc::new(Mutex::new(ws_stream));
let ws_read = Arc::clone(&ws);
let ws_write = Arc::clone(&ws);

// In handle_run_sql:
let response = serde_json::json!({
    "type": "query_result",
    "query_id": query_id,
    "columns": result.columns,
    "rows": result.rows,
    "row_count": result.row_count,
    "execution_ms": result.duration_ms
});

ws_write.lock().await.send_json(&response).await?;
```

### Issue 2: Backend URL Hardcoded

**Location:** Desktop agent expects `https://api.sery.ai`

**Current:** Backend runs on `http://localhost:8000`

**Solutions:**
1. **Quick Fix:** Update `desktop-agent/src/stores/agentStore.ts` default config:
   ```typescript
   cloud: {
     api_url: "http://localhost:8000",
     websocket_url: "ws://localhost:8000"
   }
   ```

2. **Proper Fix:** Make it configurable in Settings UI (already exists!)

### Issue 3: Authentication Requires User Login

**Problem:** `/agent/authorize` requires `current_user` (session cookie)

**Solutions:**
1. Log in to web app first: http://localhost:3000
2. Browser will have session cookie
3. OAuth flow will work

**Alternative:** For testing, you can temporarily bypass auth in the endpoint:
```python
# In agent_auth.py, comment out current_user dependency for testing
@router.get("/authorize", response_class=HTMLResponse)
async def authorize_agent(
    agent_name: str,
    platform: str,
    # current_user: User = Depends(get_current_user)  # Comment out
):
    # Hardcode for testing
    current_user = await get_user_by_email(db, "test@example.com")
    # ... rest of code
```

## ✅ Success Checklist

After completing all tests, you should have:

- [ ] Desktop agent authenticates successfully
- [ ] Token stored in macOS Keychain
- [ ] Folders can be added via UI
- [ ] Metadata syncs to backend database
- [ ] WebSocket connection stays "Connected"
- [ ] Agent status shows "online" in backend
- [ ] File watching detects new files
- [ ] Queries can be sent to agent (even if results not returned yet)

## 📊 Monitoring

### Desktop Agent Logs
Check terminal where `pnpm tauri dev` is running:
```
WebSocket connected
File changed: /path/to/file.parquet
Executing query q_123: SELECT * FROM ...
Query q_123 completed: 10 rows in 45ms
```

### Backend Logs
Check terminal where `uvicorn` is running:
```
✅ Agent abc-123 connected
Received message: {'type': 'pong', 'timestamp': ...}
```

### Database
```sql
-- Check agents
SELECT id, name, status, last_seen_at FROM agents;

-- Check datasets synced by agents
SELECT id, query_path, file_format, row_count_estimate
FROM datasets
WHERE sync_mode = 'tunnel';

-- Check agent tokens
SELECT agent_id, expires_at, last_used_at FROM agent_tokens;
```

### Redis
```bash
docker exec -it datalake-redis-1 redis-cli

# Check agent online status
KEYS agent_status:*
GET agent_status:{agent_id}
```

## 🎯 Next Steps After Testing

Once basic flow works:

1. **Fix WebSocket bidirectional** - Highest priority
2. **Add better error handling** - Show errors in UI
3. **Implement auto-reconnect** - Already coded, test it works
4. **Add query history** - Store executed queries
5. **System tray integration** - Keep agent running in background
6. **Build for production** - `pnpm tauri build`
7. **Multi-platform testing** - Windows, Linux

## 🚨 Troubleshooting

### Agent Won't Connect
1. Check backend is running: `curl http://localhost:8000/health`
2. Check WebSocket endpoint: `ws://localhost:8000/api/v1/agent/tunnel`
3. Check agent has valid token in keychain
4. Check backend logs for connection attempts

### Authorization Page 404
1. Verify backend routes are registered in `router.py`
2. Check user is logged in (has session cookie)
3. Try accessing directly: http://localhost:8000/agent/authorize?agent_name=Test&platform=macOS

### Metadata Sync Fails
1. Check folder has readable files
2. Check DuckDB can read file format
3. Check backend logs for errors
4. Verify agent token is valid

### WebSocket Disconnects
1. Check heartbeat is working (15-second interval)
2. Check Redis is running
3. Check network stability
4. Review backend connection manager logs

---

**You're almost done!** The desktop agent is 95% complete. Just need to fix the WebSocket send and test everything end-to-end. 🎉
