#!/usr/bin/env node
// Regenerate src-tauri/icons/_source-1024.png from website/public/sery-mark.svg,
// then run `pnpm tauri icon` to produce all platform icon variants.
//
// Usage: node scripts/gen-icons.js
// Requires: sharp (already in devDependencies via @tauri-apps/cli peer)
//
// The mark SVG (1024×1024 viewBox, cream petals #F9F9F7 on transparent) is
// rendered inside a purple (#5A3EA3) rounded-square background with 16%
// padding on each side — matching the iOS/macOS safe-area convention for
// app icons. Output is a 1024×1024 RGBA PNG saved as _source-1024.png.

const path = require('path');
const fs = require('fs');
const { execSync } = require('child_process');
const sharp = require('sharp');

const ROOT = path.resolve(__dirname, '..');
const WEBSITE = path.resolve(ROOT, '../website');
const MARK_SVG = path.join(WEBSITE, 'public', 'sery-mark.svg');
const OUT = path.join(ROOT, 'src-tauri', 'icons', '_source-1024.png');

const SIZE = 1024;
const PADDING = Math.round(SIZE * 0.16);   // 164px each side → mark fills ~68% of canvas
const INNER = SIZE - PADDING * 2;           // 696px mark area
const BG = '#5A3EA3';                       // brand purple

async function main() {
  if (!fs.existsSync(MARK_SVG)) {
    console.error('ERROR: sery-mark.svg not found at', MARK_SVG);
    process.exit(1);
  }

  console.log(`Reading ${MARK_SVG}`);
  const svg = fs.readFileSync(MARK_SVG);

  // Render the mark SVG at INNER×INNER
  const markPng = await sharp(svg)
    .resize(INNER, INNER, { fit: 'contain', background: { r: 0, g: 0, b: 0, alpha: 0 } })
    .png()
    .toBuffer();

  // Composite mark onto solid purple canvas
  const bgR = parseInt(BG.slice(1, 3), 16);
  const bgG = parseInt(BG.slice(3, 5), 16);
  const bgB = parseInt(BG.slice(5, 7), 16);

  await sharp({
    create: { width: SIZE, height: SIZE, channels: 4, background: { r: bgR, g: bgG, b: bgB, alpha: 255 } },
  })
    .composite([{ input: markPng, top: PADDING, left: PADDING }])
    .png()
    .toFile(OUT);

  console.log(`Wrote ${OUT} (${SIZE}×${SIZE})`);
  console.log('Running: pnpm tauri icon ...');

  execSync(`pnpm tauri icon "${OUT}"`, { cwd: ROOT, stdio: 'inherit' });
  console.log('Done. All icon variants regenerated.');
}

main().catch((e) => { console.error(e); process.exit(1); });
