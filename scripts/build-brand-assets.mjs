// Build Sery Link brand assets from the canonical 8-petal mark.
//
//   1. src/assets/sery-logo.svg
//      The in-app logo used by OnboardingWizard. Recolored to brand
//      purple #5b3ea3 (mark in solid color) so it works on any bg
//      without CSS overrides.
//
//   2. src-tauri/icons/_source-1024.png
//      A 1024×1024 white-mark-on-purple-gradient source for
//      `cargo tauri icon` to consume. Tauri then regenerates all
//      platform icons (macOS .icns, Windows .ico, iOS app icon
//      family, Android mipmap drawables, Linux PNGs).
//
//   3. src-tauri/icons/window-icon.png
//      Used by Tauri's set_icon for the OS-level window/taskbar
//      icon. Same gradient + white mark but rendered as a flat
//      256×256 PNG (no rounded square — the OS does its own
//      framing).
//
//   4. src-tauri/icons/tray-44x44.png
//      macOS menu-bar tray icon. icon_as_template=true means the
//      OS recolors it for dark/light mode, so the source must be
//      MONOCHROME — solid black mark on transparent bg, no purple,
//      no gradient. Rendered at 88×88 (2× retina) for crispness.
//
// Re-run with:    node scripts/build-brand-assets.mjs
// Then run:       cargo tauri icon src-tauri/icons/_source-1024.png

import { readFile, writeFile } from 'node:fs/promises';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');

const PURPLE = '#5b3ea3';
const PURPLE_LIGHT = '#7c3aed';
const WHITE = '#ffffff';

async function loadMarkPaths() {
  const raw = await readFile(join(ROOT, 'src', 'assets', 'sery-mark.svg'), 'utf8');
  const paths = [...raw.matchAll(/<path[^>]*\/>/g)].map((m) => m[0]);
  return paths.map((p) => p.replace(/\s*fill="[^"]*"/g, ''));
}

// In-app logo — solid #5b3ea3 mark on transparent bg, sized to
// match the prior 200×200 viewBox so existing layout (h-16 w-16
// in OnboardingWizard) doesn't shift.
function inAppLogoSvg(paths) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 1024 1024" fill="none">
  <g fill="${PURPLE}">
    ${paths.join('\n    ')}
  </g>
</svg>`;
}

// Tauri app icon source — 1024×1024, gradient purple bg, white
// mark inset by ~18% (Apple HIG / Material design sweet spot).
function tauriIconSourceSvg(paths) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1024" y2="1024" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="${PURPLE}"/>
      <stop offset="1" stop-color="${PURPLE_LIGHT}"/>
    </linearGradient>
  </defs>
  <rect width="1024" height="1024" rx="200" fill="url(#bg)"/>
  <g transform="translate(192 192) scale(0.625)" fill="${WHITE}">
    ${paths.join('\n    ')}
  </g>
</svg>`;
}

// Tray icon — monochrome black mark on transparent bg. icon_as_
// template=true on macOS means the OS recolors per dark/light
// mode, so a flat black silhouette is what we want; gradient or
// purple would look broken under the template treatment.
function trayIconSvg(paths) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="88" height="88" viewBox="0 0 1024 1024">
  <g fill="#000000">
    ${paths.join('\n    ')}
  </g>
</svg>`;
}

async function main() {
  console.log('Loading mark paths…');
  const paths = await loadMarkPaths();
  console.log(`  found ${paths.length} path elements`);

  // 1. Replace src/assets/sery-logo.svg with the new mark in brand purple.
  console.log('Writing src/assets/sery-logo.svg…');
  await writeFile(join(ROOT, 'src', 'assets', 'sery-logo.svg'), inAppLogoSvg(paths));
  console.log('  done.');

  // 2. Render the 1024 PNG source for Tauri.
  console.log('Rendering src-tauri/icons/_source-1024.png…');
  const tauriSvg = tauriIconSourceSvg(paths);
  const tauriPng = await sharp(Buffer.from(tauriSvg), { density: 384 })
    .resize(1024, 1024)
    .png()
    .toBuffer();
  await writeFile(join(ROOT, 'src-tauri', 'icons', '_source-1024.png'), tauriPng);
  console.log(`  wrote (${(tauriPng.byteLength / 1024).toFixed(1)} KB)`);

  // 3. Window icon — same composite as the Tauri source, sized
  //    down to 256×256. Used by lib.rs set_icon() for the OS-
  //    level window/taskbar icon (no OS rounding mask applied).
  console.log('Rendering src-tauri/icons/window-icon.png…');
  const winPng = await sharp(Buffer.from(tauriSvg), { density: 256 })
    .resize(256, 256)
    .png()
    .toBuffer();
  await writeFile(join(ROOT, 'src-tauri', 'icons', 'window-icon.png'), winPng);
  console.log(`  wrote (${(winPng.byteLength / 1024).toFixed(1)} KB)`);

  // 4. Tray icon — monochrome, 88×88 (2× retina), template-style
  //    black on transparent. macOS handles dark/light mode tinting.
  console.log('Rendering src-tauri/icons/tray-44x44.png…');
  const trayPng = await sharp(Buffer.from(trayIconSvg(paths)), { density: 256 })
    .resize(88, 88)
    .png()
    .toBuffer();
  await writeFile(join(ROOT, 'src-tauri', 'icons', 'tray-44x44.png'), trayPng);
  console.log(`  wrote (${(trayPng.byteLength / 1024).toFixed(1)} KB)`);

  console.log('\nNext step:');
  console.log('  cargo tauri icon src-tauri/icons/_source-1024.png');
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
