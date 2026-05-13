#!/usr/bin/env node
// Regenerate all platform icon variants from src-tauri/icons/_source-1024.png.
// Edit _source-1024.png in your design tool, then run this script.
//
// macOS dock icons look correctly sized when artwork fills ~82% of the canvas
// (~9% padding each side), matching Apple's system app convention. This script
// adds that padding before passing to `pnpm tauri icon`.
//
// Usage: node scripts/gen-icons.cjs

const path = require('path');
const fs = require('fs');
const { execSync } = require('child_process');
const sharp = require('sharp');

const ROOT = path.resolve(__dirname, '..');
const ICONS = path.join(ROOT, 'src-tauri', 'icons');
const SOURCE = path.join(ICONS, '_source-1024.png');
const PADDED = path.join(ICONS, '_padded-1024.png');

const CANVAS = 1024;
const PADDING = Math.round(CANVAS * 0.09); // 9% each side ≈ 92px
const ARTWORK = CANVAS - PADDING * 2;      // ≈ 840px

if (!fs.existsSync(SOURCE)) {
  console.error('ERROR: _source-1024.png not found at', SOURCE);
  process.exit(1);
}

async function main() {
  console.log(`Using source: ${SOURCE}`);
  console.log(`  Adding ${PADDING}px padding → artwork ${ARTWORK}×${ARTWORK} in ${CANVAS}×${CANVAS} canvas`);

  const artwork = await sharp(SOURCE).resize(ARTWORK, ARTWORK).toBuffer();

  await sharp({
    create: { width: CANVAS, height: CANVAS, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } }
  })
    .composite([{ input: artwork, top: PADDING, left: PADDING }])
    .png()
    .toFile(PADDED);

  // window-icon.png used by lib.rs via include_bytes! — not produced by tauri icon
  await sharp(PADDED).resize(256, 256).png().toFile(path.join(ICONS, 'window-icon.png'));
  console.log('  Wrote window-icon.png (256×256)');

  execSync(`pnpm tauri icon "${PADDED}"`, { cwd: ROOT, stdio: 'inherit' });

  fs.unlinkSync(PADDED);
  console.log('Done. All icon variants regenerated.');
}

main().catch((e) => { console.error(e); process.exit(1); });
