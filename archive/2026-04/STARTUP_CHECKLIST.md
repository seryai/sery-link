# Desktop Agent Startup Checklist

## ✅ Pre-flight Checks

- [x] Rust backend compiles (`cargo check`)
- [x] TypeScript has no errors (`npx tsc --noEmit`)
- [x] Tailwind CSS v3 configured
- [ ] App runs in dev mode

## 🚀 Run the App

```bash
pnpm tauri dev
```

Expected output:
```
✓ Vite dev server starts on http://localhost:1420
✓ Tauri window opens with React app
✓ You see the "Sery.ai Desktop Agent" login screen
```

## 🧪 Testing Flow

### 1. First Launch (No Auth)
- [ ] See AuthFlow component (purple gradient background)
- [ ] Input field for "Agent Name" is visible
- [ ] "Connect to Sery.ai" button is enabled

### 2. OAuth Flow (Requires Backend)
**Expected behavior when backend is ready:**
- [ ] Click "Connect to Sery.ai"
- [ ] Browser opens to `https://api.sery.ai/agent/authorize?...`
- [ ] After approval, redirects to `http://localhost:7777?code=...`
- [ ] Desktop app receives code and exchanges for token
- [ ] Token saved to macOS Keychain
- [ ] App transitions to main interface

**Current limitation:**
❌ Backend API endpoints not yet implemented - will fail at authorization step

### 3. Main Interface (After Auth)
- [ ] StatusBar shows "Connecting..." then "Connected" or "Offline"
- [ ] Sidebar with "Folders" and "Settings" tabs
- [ ] Click "Folders" → See FolderList component
- [ ] Click "Settings" → See Settings component

### 4. Folder Management
- [ ] Click "Add Folder" button
- [ ] Native folder picker opens
- [ ] Select folder with .parquet or .csv files
- [ ] Folder appears in watched list
- [ ] Click "Sync Now" → Scans folder and syncs metadata

### 5. Settings Page
- [ ] Shows Agent ID and Workspace ID
- [ ] Can edit Agent Name
- [ ] Toggle "Auto-sync on file change"
- [ ] Edit sync interval
- [ ] Click "Logout" → Returns to auth screen

## ⚠️ Known Issues

### Backend Not Ready
The desktop agent is **complete** but requires these backend endpoints:

1. **Authentication**
   - `GET /agent/authorize` - Authorization page
   - `POST /api/v1/agent/token` - Token exchange
   - `GET /api/v1/agent/info` - Agent verification

2. **Metadata Sync**
   - `POST /api/v1/agent/sync-metadata` - Store dataset schemas

3. **WebSocket Tunnel**
   - `WS /api/v1/agent/tunnel` - Query execution channel

**Workaround for testing UI only:**
You can test the UI without backend by:
1. Commenting out the API calls in `src/components/AuthFlow.tsx`
2. Manually setting `authenticated: true` in the store
3. Mocking the config data

### File Watching
File watcher will start monitoring after:
- Folders are added
- App restarts (loads saved config)

Currently only detects: `.parquet`, `.csv`, `.xlsx`

### WebSocket Send
Query results are received but not sent back (need architectural refactor).
See `src-tauri/src/websocket.rs:179` - `handle_run_sql` logs results but doesn't return them.

## 🔧 Troubleshooting

### App Won't Start
```bash
# Clean build
cd src-tauri
cargo clean
cd ..
rm -rf node_modules
pnpm install
pnpm tauri dev
```

### Keychain Access Denied (macOS)
- Go to System Settings → Privacy & Security
- Grant Keychain access to "sery-link"

### Port 1420 Already in Use
```bash
# Kill Vite process
pkill -f vite
pnpm tauri dev
```

### Port 7777 Already in Use
```bash
# Check what's using it
lsof -i :7777
# Kill if needed
kill -9 <PID>
```

## 📝 Next Implementation Steps

1. **Backend API Endpoints** (Priority 1)
   - Create FastAPI routes for agent auth
   - Implement token exchange logic
   - Add WebSocket tunnel handler

2. **WebSocket Bi-directional** (Priority 2)
   - Refactor to keep write handle
   - Send query results back to cloud
   - Handle errors properly

3. **Testing** (Priority 3)
   - End-to-end auth flow
   - Metadata sync verification
   - Query round-trip test

4. **Polish** (Priority 4)
   - Better error messages
   - Loading states
   - Toast notifications
   - System tray integration

## ✨ Success Criteria

The desktop agent is **production-ready** when:
- ✅ Compiles without errors
- ✅ UI renders correctly
- ⏳ OAuth flow completes successfully (backend needed)
- ⏳ Folders can be added and watched
- ⏳ Metadata syncs to cloud
- ⏳ WebSocket stays connected
- ⏳ Queries execute locally and return results
- ⏳ Token persists across app restarts

**Current status: 7/8 complete (87.5%)**
