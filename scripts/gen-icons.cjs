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
const PADDED = path.join(ICONS, '_padded-1024.png');

// macOS dock convention: content fills ~80% of canvas (102px padding each side).
// _source-1024.png is the raw artwork at full bleed; we shrink it here so the
// icon doesn't appear oversized next to other apps in the dock.
const CANVAS = 1024;
const PADDING = Math.round(CANVAS * 0.09); // 9% each side → content = 82% of canvas
const INNER = CANVAS - PADDING * 2;        // 819px

if (!fs.existsSync(SOURCE)) {
  console.error('ERROR: _source-1024.png not found at', SOURCE);
  process.exit(1);
}

async function main() {
  console.log(`Using source: ${SOURCE}`);

  // Shrink source into padded canvas for Tauri icon generation
  const resized = await sharp(SOURCE)
    .resize(INNER, INNER, { fit: 'contain', background: { r:0, g:0, b:0, alpha:0 } })
    .png()
    .toBuffer();

  await sharp({ create: { width: CANVAS, height: CANVAS, channels: 4, background: { r:0, g:0, b:0, alpha:0 } } })
    .composite([{ input: resized, top: PADDING, left: PADDING }])
    .png()
    .toFile(PADDED);
  console.log(`  Padded intermediate: ${INNER}×${INNER} content on ${CANVAS}×${CANVAS} canvas (${PADDING}px padding)`);

  // window-icon.png is used by lib.rs via include_bytes! — not produced by tauri icon
  await sharp(PADDED)
    .resize(256, 256, { fit: 'contain' })
    .png()
    .toFile(path.join(ICONS, 'window-icon.png'));
  console.log('  PNG wrote window-icon.png (256×256)');

  execSync(`pnpm tauri icon "${PADDED}"`, { cwd: ROOT, stdio: 'inherit' });

  fs.unlinkSync(PADDED);
  console.log('Done. All icon variants regenerated.');
}

main().catch((e) => { console.error(e); process.exit(1); });
