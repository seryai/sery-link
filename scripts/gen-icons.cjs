#!/usr/bin/env node
// Regenerate all platform icon variants from src-tauri/icons/_source-1024.png.
// Edit _source-1024.png manually, then run this script.
//
// Usage: node scripts/gen-icons.cjs

const path = require('path');
const fs = require('fs');
const { execSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');
const SOURCE = path.join(ROOT, 'src-tauri', 'icons', '_source-1024.png');

if (!fs.existsSync(SOURCE)) {
  console.error('ERROR: _source-1024.png not found at', SOURCE);
  process.exit(1);
}

console.log(`Using source: ${SOURCE}`);
execSync(`pnpm tauri icon "${SOURCE}"`, { cwd: ROOT, stdio: 'inherit' });
console.log('Done. All icon variants regenerated.');
