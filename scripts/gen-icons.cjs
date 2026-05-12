#!/usr/bin/env node
// Regenerate all platform icon variants from src-tauri/icons/_source-1024.png.
// Edit _source-1024.png to control padding/sizing, then run this script.
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

// _source-1024.png has pre-rounded corners (transparent). macOS treats
// transparent corner pixels as empty space, making the icon look smaller
// than apps that ship a full-bleed square (which macOS rounds itself).
// Fix: composite the source onto a solid background so corners are filled,
// then let macOS apply its own rounding at render time.
async function solidBackground(sourcePath) {
  const { data, info } = await sharp(sourcePath).raw().toBuffer({ resolveWithObject: true });
  // Sample background color from a known-opaque pixel near the top edge
  const i = (30 * info.width + 512) * info.channels;
  const bg = { r: data[i], g: data[i+1], b: data[i+2], alpha: 255 };

  return sharp({ create: { width: 1024, height: 1024, channels: 4, background: bg } })
    .composite([{ input: sourcePath }])
    .png()
    .toBuffer();
}

async function main() {
  console.log(`Using source: ${SOURCE}`);

  const fullBleed = await solidBackground(SOURCE);
  const FULL = path.join(ICONS, '_fullbleed-1024.png');
  await sharp(fullBleed).toFile(FULL);
  console.log('  Filled transparent corners with background color');

  // window-icon.png is used by lib.rs via include_bytes! — not produced by tauri icon
  await sharp(fullBleed)
    .resize(256, 256, { fit: 'contain' })
    .png()
    .toFile(path.join(ICONS, 'window-icon.png'));
  console.log('  PNG wrote window-icon.png (256×256)');

  execSync(`pnpm tauri icon "${FULL}"`, { cwd: ROOT, stdio: 'inherit' });
  fs.unlinkSync(FULL);
  console.log('Done. All icon variants regenerated.');
}

main().catch((e) => { console.error(e); process.exit(1); });
