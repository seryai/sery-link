#!/usr/bin/env node
// Regenerate all platform icon variants from src-tauri/icons/_source-1024.png.
// Edit _source-1024.png to control artwork, then run this script.
//
// PADDING: artwork is shrunk to (1 - PADDING*2) of the canvas, padded with
// the icon's own background color. macOS gets a full-bleed square and applies
// its own rounded-corner mask. Adjust PADDING to match other dock icons visually.
//
// Usage: node scripts/gen-icons.cjs

const path = require('path');
const fs = require('fs');
const { execSync } = require('child_process');
const sharp = require('sharp');

const ROOT = path.resolve(__dirname, '..');
const ICONS = path.join(ROOT, 'src-tauri', 'icons');
const SOURCE = path.join(ICONS, '_source-1024.png');
const CANVAS = 1024;
const PADDING = Math.round(CANVAS * 0.10); // tune this: 0.08=bigger, 0.12=smaller

if (!fs.existsSync(SOURCE)) {
  console.error('ERROR: _source-1024.png not found at', SOURCE);
  process.exit(1);
}

async function main() {
  console.log(`Using source: ${SOURCE}`);

  // Sample background color from top-left corner area
  const { data, info } = await sharp(SOURCE).raw().toBuffer({ resolveWithObject: true });
  const i = (5 * info.width + 5) * info.channels;
  const bg = { r: data[i], g: data[i+1], b: data[i+2], alpha: 255 };

  const INNER = CANVAS - PADDING * 2;
  const artwork = await sharp(SOURCE)
    .resize(INNER, INNER, { fit: 'contain' })
    .png()
    .toBuffer();

  const padded = await sharp({
    create: { width: CANVAS, height: CANVAS, channels: 4, background: bg },
  })
    .composite([{ input: artwork, top: PADDING, left: PADDING }])
    .png()
    .toBuffer();

  const PADDED = path.join(ICONS, '_padded-1024.png');
  await sharp(padded).toFile(PADDED);
  console.log(`  Padded: ${INNER}×${INNER} artwork on ${CANVAS}×${CANVAS} canvas (${PADDING}px / ${(PADDING/CANVAS*100).toFixed(0)}% padding)`);

  // window-icon.png used by lib.rs via include_bytes! — not produced by tauri icon
  await sharp(padded).resize(256, 256).png().toFile(path.join(ICONS, 'window-icon.png'));
  console.log('  PNG wrote window-icon.png (256×256)');

  execSync(`pnpm tauri icon "${PADDED}"`, { cwd: ROOT, stdio: 'inherit' });
  fs.unlinkSync(PADDED);
  console.log('Done. All icon variants regenerated.');
}

main().catch((e) => { console.error(e); process.exit(1); });
