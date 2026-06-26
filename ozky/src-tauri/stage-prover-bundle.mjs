// Assembles the no-Docker prover bundle that ships inside the Tauri app, so an end-user
// install proves with NO Docker and NO dev `ozky.config.json` absolute paths.
//
//   node stage-prover-bundle.mjs
//
// Copies into src-tauri/prover-bundle/ (gitignored — built from large artifacts):
//   prover/   ← the ozky-prover SEA binary + the WASM/worker blobs it loads at runtime
//   zk/       ← the 9 compiled circuit JSONs + their FROZEN VKs, mirroring the repo layout
//               (circuits/<name>/target/<name>.json, contracts/frozen_vks/<name>/vk)
//
// tauri.conf.json bundles `prover-bundle/**/*` as resources; at startup lib.rs points
// OZKY_PROVER_BIN/OZKY_PROVER_ASSETS at prover/ and OZKY_REPO_ROOT at zk/. The sidecar
// then resolves circuit + VK from there (read-only) and stages proving in a temp dir.
//
// Runs in tauri.conf.json `beforeBuildCommand` (after `npm run build`). Fails loudly if a
// required artifact is missing — a partial bundle would ship a wallet that can't prove.

import { mkdirSync, copyFileSync, rmSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const srcTauri = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(srcTauri, '..', '..'); // src-tauri -> ozky -> repo root (D:\ozky)
const dist = join(repoRoot, 'prover-sidecar', 'dist');
const out = join(srcTauri, 'prover-bundle');

// The 9 production circuits the wallet proves against (must match proving.rs Circuit::name).
const CIRCUITS = [
  'deposit', 'transfer', 'transfer4', 'withdraw', 'split',
  'escrow_contribute', 'escrow_payout', 'channel_close', 'shielded_swap',
];

// Runtime sidecar files: the SEA binary + the WASM/workers it reads from its own dir.
const proverExe = process.platform === 'win32' ? 'ozky-prover.exe' : 'ozky-prover';
const PROVER_FILES = [
  proverExe,
  'acvm_js_bg.wasm',
  'noirc_abi_wasm_bg.wasm',
  'barretenberg-threads.wasm.gz',
  'main.worker.js',
  'thread.worker.js',
];

function need(path, hint) {
  if (!existsSync(path)) {
    console.error(`missing required artifact: ${path}\n  → ${hint}`);
    process.exit(1);
  }
}

// Fresh bundle each build (avoid shipping stale circuits/VKs).
rmSync(out, { recursive: true, force: true });

// 1. prover/ — SEA binary + WASM/workers.
const proverOut = join(out, 'prover');
mkdirSync(proverOut, { recursive: true });
for (const f of PROVER_FILES) {
  const src = join(dist, f);
  need(src, `build it first: cd prover-sidecar && node build.mjs --sea`);
  copyFileSync(src, join(proverOut, f));
}

// 2. zk/ — compiled circuit JSON + frozen VK per circuit, mirroring the repo layout.
for (const name of CIRCUITS) {
  const circuitJson = join(repoRoot, 'circuits', name, 'target', `${name}.json`);
  need(circuitJson, `compile it: docker compose -f compose.zk.yaml run --rm zk bash -c 'cd circuits/${name} && nargo compile'`);
  const jsonOut = join(out, 'zk', 'circuits', name, 'target');
  mkdirSync(jsonOut, { recursive: true });
  copyFileSync(circuitJson, join(jsonOut, `${name}.json`));

  const vk = join(repoRoot, 'contracts', 'frozen_vks', name, 'vk');
  need(vk, `the frozen VK for ${name} is missing — it must not be regenerated`);
  const vkOut = join(out, 'zk', 'contracts', 'frozen_vks', name);
  mkdirSync(vkOut, { recursive: true });
  copyFileSync(vk, join(vkOut, 'vk'));
}

console.log(`staged prover bundle → ${out} (1 binary + ${PROVER_FILES.length - 1} blobs + ${CIRCUITS.length} circuits/VKs)`);
