// Ensures src-tauri/prover-bundle/ exists so tauri.conf.json's `prover-bundle/**/*`
// resource glob resolves during `tauri dev`.
//
//   node dev-prover-bundle.mjs
//
// The real bundle (the 84MB SEA prover + circuit JSONs/VKs) is staged only at build time by
// stage-prover-bundle.mjs. In dev the wallet proves via the absolute paths in ozky.config.json
// (OZKY_PROVER_BIN/OZKY_PROVER_ASSETS/OZKY_REPO_ROOT), so the bundle resource is never read — a
// single placeholder file is enough to satisfy the glob without staging the heavy artifacts.
//
// Runs in tauri.conf.json `beforeDevCommand` (before `npm run dev`). No-op when the directory is
// already populated (a real build leaves the full bundle in place; this won't clobber it).

import { mkdirSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const srcTauri = dirname(fileURLToPath(import.meta.url));
const out = join(srcTauri, 'prover-bundle');

// Already staged (real bundle or a prior placeholder) → leave it untouched.
if (existsSync(out) && readdirSync(out).length > 0) process.exit(0);

mkdirSync(out, { recursive: true });
// Non-dot filename so the `prover-bundle/**/*` glob matches it unconditionally (a leading-dot
// name is not reliably matched by `*`).
writeFileSync(
  join(out, 'DEV_PLACEHOLDER.txt'),
  'Placeholder so the tauri.conf.json `prover-bundle/**/*` resource glob resolves during ' +
    '`tauri dev`.\nThe real bundle is staged at build time by stage-prover-bundle.mjs; in dev the ' +
    'prover comes from\nozky.config.json (OZKY_PROVER_BIN). Safe to delete.\n',
);
console.log('created prover-bundle/ dev placeholder (real bundle is staged at build time)');
