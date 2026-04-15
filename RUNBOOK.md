# Sery Link - Developer Runbook

Complete guide to building, running, and testing Sery Link with document support.

---

## Prerequisites

### Required Software

```bash
# Verify installations
node --version        # Need: 18+
pnpm --version        # Need: 9+
cargo --version       # Need: 1.88+
python3 --version     # Need: 3.11+
```

### Install Missing Dependencies

**Node.js & pnpm:**
```bash
# Install Node.js 22 LTS
# macOS: brew install node@22
# Windows: Download from nodejs.org

# Install pnpm
npm install -g pnpm
```

**Rust:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

**Python 3.11+ (for sidecar building):**
```bash
# macOS
brew install python@3.11

# Windows
# Download from python.org

# Verify
python3 --version  # Should be 3.11 or higher
```

---

## Installation for End Users

**TL;DR:** Users download ONE installer (DMG/MSI/AppImage) and install ONE app. The MarkItDown sidecar is bundled inside automatically - users never see it.

**What users get:**
- macOS: Download `Sery Link.dmg`, drag to Applications, done
- Windows: Download `Sery Link.msi`, run installer, done
- Linux: Download `sery-link.AppImage`, make executable, done

**What's inside the app:**
- Main Sery Link binary (~10 MB)
- MarkItDown sidecar binary (~180 MB) - bundled automatically
- Tauri manages the sidecar lifecycle - users never interact with it directly

**Total installed size:** ~200 MB (compressed installers are ~60-80 MB)

---

## Quick Start (Development)

### 1. Clone and Install

```bash
cd /path/to/sery-link
pnpm install
```

### 2. Build the Sidecar (First Time)

The MarkItDown sidecar must be built before running the app:

```bash
cd sidecar
python3 build.py
```

**What this does:**
- Creates Python virtual environment
- Installs dependencies (markitdown, magika, etc.)
- Compiles to standalone binary with PyInstaller
- Creates platform-specific binary name (e.g., `markitdown-sidecar-aarch64-apple-darwin`)
- Output: `sidecar/dist/markitdown-sidecar` (~180 MB)

**Expected output:**
```
✓ Created virtual environment
✓ Installed dependencies
✓ Building standalone binary with PyInstaller...
✓ Binary created: dist/markitdown-sidecar-aarch64-apple-darwin (179 MB)
✓ Generic symlink created: dist/markitdown-sidecar
```

**Troubleshooting:**
- If `python3` not found: Use `python` or specify full path
- If PyInstaller fails: Try `pip install --upgrade pyinstaller`
- If magika data files missing: The build.py script handles this automatically

### 3. Run Development Server

```bash
pnpm tauri dev
```

**What happens:**
- Vite dev server starts (React frontend)
- Rust backend compiles
- Tauri window opens with hot reload
- Sidecar binary is bundled automatically

**First run may take 2-3 minutes** (Rust compilation + dependencies)

---

## Building for Production

### macOS

#### Universal Binary (Intel + Apple Silicon)

```bash
# Build the app
pnpm tauri build --target universal-apple-darwin

# Output locations:
# DMG installer: src-tauri/target/universal-apple-darwin/release/bundle/dmg/Sery Link_<version>_universal.dmg
# .app bundle:   src-tauri/target/universal-apple-darwin/release/bundle/macos/Sery Link.app
```

#### Architecture-Specific Builds

**Apple Silicon only (smaller, faster):**
```bash
pnpm tauri build --target aarch64-apple-darwin
# Output: src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/
```

**Intel only:**
```bash
pnpm tauri build --target x86_64-apple-darwin
# Output: src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/
```

### Windows

**Prerequisites:**
```powershell
# Install WebView2 runtime (required by Tauri)
# Download from: https://developer.microsoft.com/en-us/microsoft-edge/webview2/
```

**Build:**
```bash
# Ensure sidecar is built for Windows first
cd sidecar
python build.py  # Creates markitdown-sidecar-x86_64-pc-windows-msvc.exe

# Build the app
pnpm tauri build

# Output locations:
# MSI installer: src-tauri/target/release/bundle/msi/Sery Link_<version>_x64_en-US.msi
# .exe:          src-tauri/target/release/Sery Link.exe
```

### Linux

**Prerequisites:**
```bash
# Debian/Ubuntu
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev

# Fedora
sudo dnf install webkit2gtk4.1-devel \
  openssl-devel \
  curl \
  wget \
  file \
  libappindicator-gtk3-devel \
  librsvg2-devel

# Arch
sudo pacman -S webkit2gtk-4.1 \
  base-devel \
  curl \
  wget \
  file \
  openssl \
  appmenu-gtk-module \
  gtk3 \
  libappindicator-gtk3 \
  librsvg \
  libvips
```

**Build:**
```bash
# Build sidecar for Linux
cd sidecar
python3 build.py  # Creates markitdown-sidecar-x86_64-unknown-linux-gnu

# Build the app
pnpm tauri build

# Output locations:
# .deb package:  src-tauri/target/release/bundle/deb/sery-link_<version>_amd64.deb
# .AppImage:     src-tauri/target/release/bundle/appimage/sery-link_<version>_amd64.AppImage
```

---

## Testing Document Support

### 1. Prepare Test Files

Create a test folder with sample documents:

```bash
mkdir -p ~/Documents/SeryTest
cd ~/Documents/SeryTest

# Download or create test files:
# - resume.docx       (Word document)
# - presentation.pptx (PowerPoint)
# - report.html       (HTML file)
# - guide.pdf         (PDF)
# - analysis.ipynb    (Jupyter notebook)
```

### 2. Test the Sidecar Manually

Before testing in the app, verify the sidecar works:

```bash
cd sery-link/sidecar

# Test with a DOCX file
echo "/path/to/resume.docx" | ./dist/markitdown-sidecar

# Expected output (JSON):
{
  "success": true,
  "markdown": "# Resume\n\n**John Doe**\n...",
  "error": null
}

# Test with an unsupported file
echo "/path/to/image.jpg" | ./dist/markitdown-sidecar

# Expected output:
{
  "success": false,
  "markdown": null,
  "error": "Conversion failed: Unsupported file type"
}
```

### 3. Test in Development App

```bash
pnpm tauri dev
```

**Steps:**
1. Click "Login to Sery.ai" in the menu bar icon
2. Authenticate via browser OAuth
3. Click "+ Add Folder"
4. Select your test folder: `~/Documents/SeryTest`
5. Wait for scan to complete (~5-10 seconds)
6. Check the "Folders" tab:
   - Should show 5 files detected
   - Document files show as "DOCX", "PPTX", etc.
   - File sizes displayed correctly

**Verify in backend logs:**
```bash
# On the backend server (api), check logs:
# You should see:
# "Received dataset metadata from agent: resume.docx (DOCX, 24576 bytes)"
# "Document markdown extracted: 1234 chars"
```

### 4. Test Document Querying

Once documents are synced to the backend:

1. Open the Sery.ai web dashboard
2. Navigate to Chat
3. Ask: "What's in the resume.docx file?"
4. The AI should:
   - Search and find the document dataset
   - Read the markdown content via the `read_document` tool
   - Summarize the resume contents

**Expected flow (check backend logs):**
```
[Agent] Searching for: resume.docx
[Agent] Found dataset: resume.docx (tunnel mode)
[Agent] Calling tool: read_document
[Tunnel] Sending query to agent: <agent_id>
[Agent] Returning document text: 1234 chars
[Agent] Response: "This resume is for John Doe, a software engineer with 5 years..."
```

---

## Development Workflows

### Rebuilding the Sidecar

**When to rebuild:**
- After updating `sidecar/requirements.txt`
- After changing `sidecar/markitdown_worker.py`
- After updating to a new MarkItDown version

**How to rebuild:**
```bash
cd sidecar
rm -rf dist/ build/ venv/  # Clean previous build
python3 build.py            # Rebuild
```

**Quick rebuild (if only worker.py changed):**
```bash
cd sidecar
source venv/bin/activate
pyinstaller markitdown-sidecar.spec
```

### Hot Reload (Frontend Only)

The frontend supports hot reload, but Rust changes require restart:

- **Frontend changes (.tsx, .ts, .css):** Auto-reloads ✓
- **Rust changes (.rs):** Restart `pnpm tauri dev` ✗

### Checking Sidecar Integration

Verify the sidecar is properly bundled in the final app:

```bash
# macOS - Check .app bundle
cd src-tauri/target/release/bundle/macos
ls -lh "Sery Link.app/Contents/MacOS/"
# Should show: Sery Link (main binary ~10 MB)
#              markitdown-sidecar-aarch64-apple-darwin (~180 MB)

# Windows - Check exe directory
cd src-tauri/target/release
dir
# Should show: Sery Link.exe (~10 MB)
#              markitdown-sidecar-x86_64-pc-windows-msvc.exe (~180 MB)

# Linux - Check AppImage or deb
# Extract AppImage:
./sery-link_*.AppImage --appimage-extract
ls squashfs-root/usr/bin/
# Should show: sery-link (~10 MB)
#              markitdown-sidecar-x86_64-unknown-linux-gnu (~180 MB)
```

**Important:** The sidecar is bundled automatically by Tauri during build. End users never see or interact with it - it's an internal implementation detail. The installer packages everything together.

---

## Configuration

### Tauri Config (`src-tauri/tauri.conf.json`)

**Sidecar bundling:**
```json
{
  "bundle": {
    "externalBin": [
      "../sidecar/dist/markitdown-sidecar"
    ]
  }
}
```

**What this does:**
- Automatically includes sidecar in builds (DMG/MSI/AppImage)
- Tauri resolves platform-specific binary name at build time
- Binary placed next to main executable inside the app bundle
- Users only see ONE app to install, sidecar is invisible

### Environment Variables

**Backend URL (for OAuth and sync):**
```bash
# Development (default)
API_URL=http://localhost:8000

# Production
API_URL=https://api.sery.ai
```

Set in `src/config.ts` or via environment during build.

---

## Troubleshooting

### Sidecar Issues

**Problem:** "Sidecar binary not found"

**Solution:**
```bash
# Verify sidecar exists
ls -lh sidecar/dist/markitdown-sidecar*

# If missing, rebuild
cd sidecar
python3 build.py

# Verify platform-specific name exists
# macOS: markitdown-sidecar-aarch64-apple-darwin (Apple Silicon)
#        markitdown-sidecar-x86_64-apple-darwin (Intel)
# Windows: markitdown-sidecar-x86_64-pc-windows-msvc.exe
# Linux: markitdown-sidecar-x86_64-unknown-linux-gnu
```

---

**Problem:** "Sidecar conversion failed" in logs

**Check:**
1. File format supported? (DOCX, PPTX, HTML, PDF, IPYNB only)
2. File corrupted?
3. File too large? (>50 MB rejected)

**Debug:**
```bash
# Test sidecar directly
echo "/path/to/problem-file.docx" | sidecar/dist/markitdown-sidecar

# Check error message in JSON output
```

---

**Problem:** "Data type 'str' not recognized" (Pandas 3.x error)

This was already fixed in the scanner.rs implementation. If you see this:
- Verify you're on the latest commit
- The scanner uses PyArrow tables, not Pandas DataFrames

---

### Build Issues

**Problem:** "PyInstaller: module 'magika' not found"

**Solution:**
The `build.py` script already handles this via the custom `.spec` file. If you still see this:
```bash
cd sidecar
rm -rf build/ dist/ venv/
python3 build.py  # Full clean rebuild
```

---

**Problem:** "Tauri build fails: resource path doesn't exist"

**Solution:**
```bash
# Ensure platform-specific binary exists
cd sidecar/dist
ls -lh

# Should see BOTH:
# - markitdown-sidecar (generic symlink)
# - markitdown-sidecar-<platform-triple> (platform-specific)

# If missing, rebuild sidecar
cd ..
python3 build.py
```

---

**Problem:** "Rust compilation fails"

**Common causes:**
1. **Missing system libraries (Linux):**
   ```bash
   # Install webkit2gtk
   sudo apt install libwebkit2gtk-4.1-dev
   ```

2. **Outdated Rust:**
   ```bash
   rustup update
   ```

3. **Cargo cache corrupted:**
   ```bash
   cargo clean
   pnpm tauri dev
   ```

---

### Runtime Issues

**Problem:** Documents not appearing in folder scan

**Check:**
1. File watcher running? (Check logs)
2. File extension recognized? (DOCX, PPTX, HTML, PDF, IPYNB only)
3. Sidecar conversion succeeded? (Check conversion logs)

**Debug:**
```bash
# Enable verbose logging
RUST_LOG=debug pnpm tauri dev

# Look for:
# "Detected document file: resume.docx"
# "Trying sidecar conversion..."
# "Sidecar conversion successful: 1234 chars"
```

---

**Problem:** OAuth callback timeout

**Solution:**
```bash
# Verify local callback server starts
# Should see in logs: "OAuth callback server listening on 127.0.0.1:7777"

# Check firewall isn't blocking port 7777
# macOS: System Preferences → Security → Firewall
# Windows: Windows Defender Firewall
```

---

## File Structure Reference

```
sery-link/
├── sidecar/                              # Document processing sidecar
│   ├── markitdown_worker.py              # Python worker script
│   ├── build.py                          # PyInstaller build script
│   ├── requirements.txt                  # Python dependencies
│   ├── markitdown-sidecar.spec           # PyInstaller config (auto-generated)
│   ├── dist/                             # Build output
│   │   ├── markitdown-sidecar            # Generic symlink
│   │   └── markitdown-sidecar-<triple>   # Platform-specific binary (~180 MB)
│   └── venv/                             # Python virtual environment (gitignored)
├── src-tauri/
│   ├── src/
│   │   ├── scanner.rs                    # File scanning + sidecar integration
│   │   ├── auth.rs                       # OAuth loopback
│   │   ├── websocket.rs                  # WebSocket tunnel
│   │   ├── duckdb_engine.rs              # Query execution
│   │   └── ...
│   ├── tauri.conf.json                   # Tauri configuration (includes sidecar bundling)
│   └── Cargo.toml                        # Rust dependencies
├── src/                                  # React frontend
│   ├── App.tsx
│   ├── components/
│   └── ...
├── RUNBOOK.md                            # This file
├── IMPLEMENTATION_STATUS.md              # Feature completion status
├── SIDECAR_IMPLEMENTATION.md             # Architecture details
├── README.md                             # Quick start guide
└── package.json                          # Node.js dependencies
```

---

## Performance Notes

### Sidecar Binary Size

- **Size:** ~180 MB (bundled Python + dependencies)
- **Why so large?** Includes entire Python runtime + MarkItDown + magika models
- **Compression:** DMG/MSI installers compress well (~60 MB final)
- **Optimization:** Already uses `--onefile` and strips debug symbols

### Conversion Speed

| File Type | Avg Size | Conversion Time |
|-----------|----------|-----------------|
| DOCX      | 50 KB    | ~200 ms         |
| PPTX      | 2 MB     | ~500 ms         |
| HTML      | 100 KB   | ~100 ms         |
| PDF       | 1 MB     | ~800 ms         |
| IPYNB     | 500 KB   | ~150 ms         |

**Bottleneck:** Subprocess spawn overhead (~100 ms per file)

**Optimization ideas (future):**
- Keep sidecar process alive (daemon mode)
- Batch multiple files in one call
- Cache converted markdown

---

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Build Sery Link

on:
  push:
    branches: [main, feat/*]

jobs:
  build-macos:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v3

      - name: Setup Node
        uses: actions/setup-node@v3
        with:
          node-version: 22

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install pnpm
        run: npm install -g pnpm

      - name: Install dependencies
        run: pnpm install

      - name: Build sidecar
        run: |
          cd sidecar
          python3 build.py

      - name: Build Tauri app
        run: pnpm tauri build --target universal-apple-darwin

      - name: Upload DMG
        uses: actions/upload-artifact@v3
        with:
          name: Sery-Link-macOS
          path: src-tauri/target/universal-apple-darwin/release/bundle/dmg/*.dmg
```

---

## Release Checklist

Before releasing a new version:

- [ ] Update version in `src-tauri/tauri.conf.json`
- [ ] Update version in `package.json`
- [ ] Rebuild sidecar: `cd sidecar && python3 build.py`
- [ ] Test document conversion with all supported formats
- [ ] Test OAuth flow end-to-end
- [ ] Test WebSocket tunnel connection
- [ ] Test file watcher detects changes
- [ ] Run production build: `pnpm tauri build`
- [ ] Test installer on clean machine
- [ ] Verify sidecar bundled correctly (check .app/exe/AppImage contents)
- [ ] Update CHANGELOG.md
- [ ] Create git tag: `git tag -a v1.0.0 -m "Release 1.0.0"`
- [ ] Push tag: `git push origin v1.0.0`

---

## Getting Help

**Documentation:**
- [Tauri Docs](https://v2.tauri.app/)
- [MarkItDown GitHub](https://github.com/microsoft/markitdown)
- [PyInstaller Manual](https://pyinstaller.org/)

**Issues:**
- GitHub Issues: https://github.com/seryai/sery-link/issues
- Internal Slack: #sery-link-dev

**Logs:**
- macOS: `~/Library/Logs/ai.sery.link/`
- Windows: `%APPDATA%\ai.sery.link\logs\`
- Linux: `~/.local/share/ai.sery.link/logs/`

---

**Last Updated:** April 14, 2026
**Maintainers:** Sery.ai Engineering Team
