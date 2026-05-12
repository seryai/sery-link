#!/usr/bin/env node
// Regenerate all platform icon variants from src-tauri/icons/_source-1024.png.
// Edit _source-1024.png manually, then run this script.
//
// Usage: node scripts/gen-icons.cjs

const path = require('path');
const fs = require('fs');
const { execSync } = require('child_process');
const sharp = require('sharp');

const ROOT = path.resolve(__dirname, '..');
const ICONS = path.join(ROOT, 'src-tauri', 'icons');
const SOURCE = path.join(ICONS, '_source-1024.png');

if (!fs.existsSync(SOURCE)) {
  console.error('ERROR: _source-1024.png not found at', SOURCE);
  process.exit(1);
}

async function main() {
  console.log(`Using source: ${SOURCE}`);

  // pnpm tauri icon generates standard sizes but not window-icon.png
  // (used by lib.rs as the window titlebar icon via include_bytes!)
  await sharp(SOURCE)
    .resize(256, 256, { fit: 'contain' })
    .png()
    .toFile(path.join(ICONS, 'window-icon.png'));
  console.log('  PNG wrote window-icon.png (256×256)');

  execSync(`pnpm tauri icon "${SOURCE}"`, { cwd: ROOT, stdio: 'inherit' });
  console.log('Done. All icon variants regenerated.');
}

main().catch((e) => { console.error(e); process.exit(1); });
