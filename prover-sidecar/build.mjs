// Bundles the sidecar into a single CJS file + the WASM blobs it loads at runtime,
// then (optionally) packs it into a self-contained Node SEA executable.
//
//   node build.mjs          # → dist/prove.cjs + dist/*.wasm[.gz]
//   node build.mjs --sea    # also → dist/ozky-prover<.exe> (single binary, threads:1)
//
// The three WASM modules (bb.js + noir_js's acvm_js & noirc_abi) are read from disk at
// runtime via __dirname (noir) / wasmPath (bb), so they ship NEXT TO the bundle/exe.

import * as esbuild from 'esbuild';
import { execFileSync } from 'node:child_process';
import { mkdirSync, copyFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

const dist = 'dist';
mkdirSync(dist, { recursive: true });

await esbuild.build({
  entryPoints: ['prove.mjs'],
  bundle: true,
  platform: 'node',
  format: 'cjs',
  target: 'node22',
  outfile: join(dist, 'prove.cjs'),
  logLevel: 'info',
});

// bb.js always hosts the WASM in a worker thread (and spawns more for threads>1).
// createMainWorker/createThreadWorker load these by name from the bundle's dir, so
// bundle them as siblings. ESM .js (the package is type:module) so Node parses them.
const bbWasm = 'node_modules/@aztec/bb.js/dest/node/barretenberg_wasm';
await esbuild.build({
  entryPoints: {
    'main.worker': `${bbWasm}/barretenberg_wasm_main/factory/node/main.worker.js`,
    'thread.worker': `${bbWasm}/barretenberg_wasm_thread/factory/node/thread.worker.js`,
  },
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node22',
  outdir: dist,
  // ESM output, but some deps (debug) do dynamic require() — give them a working one.
  banner: { js: "import { createRequire as __cr } from 'module'; const require = __cr(import.meta.url);" },
  logLevel: 'info',
});

// WASM blobs loaded at runtime (not import-bundled) — copy them beside the bundle.
const assets = {
  'node_modules/@aztec/bb.js/dest/node/barretenberg_wasm/barretenberg-threads.wasm.gz':
    'barretenberg-threads.wasm.gz',
  'node_modules/@noir-lang/acvm_js/nodejs/acvm_js_bg.wasm': 'acvm_js_bg.wasm',
  'node_modules/@noir-lang/noirc_abi/nodejs/noirc_abi_wasm_bg.wasm': 'noirc_abi_wasm_bg.wasm',
};
for (const [src, name] of Object.entries(assets)) copyFileSync(src, join(dist, name));
console.log('bundled dist/prove.cjs + 3 wasm assets');

if (process.argv.includes('--sea')) {
  // Node Single Executable Application: blob from the bundle, injected into a copy
  // of the node binary. SEA mains run as CommonJS — hence the CJS bundle above.
  const seaConfig = {
    main: join(dist, 'prove.cjs'),
    output: join(dist, 'prove.blob'),
    disableExperimentalSEAWarning: true,
  };
  writeFileSync(join(dist, 'sea-config.json'), JSON.stringify(seaConfig, null, 2));
  execFileSync(process.execPath, ['--experimental-sea-config', join(dist, 'sea-config.json')], {
    stdio: 'inherit',
  });

  const exe = join(dist, process.platform === 'win32' ? 'ozky-prover.exe' : 'ozky-prover');
  copyFileSync(process.execPath, exe);

  const isMac = process.platform === 'darwin';
  // macOS ships a signed `node`; its signature must be stripped before postject
  // mutates the binary and re-applied (ad-hoc) afterwards, and postject needs an
  // explicit Mach-O segment name. No-ops on Windows/Linux.
  if (isMac) execFileSync('codesign', ['--remove-signature', exe], { stdio: 'inherit' });

  const postject = join('node_modules', 'postject', 'dist', 'cli.js');
  if (!existsSync(postject)) {
    console.error('postject not installed; run: npm i -D postject');
    process.exit(1);
  }
  execFileSync(
    process.execPath,
    [
      postject, exe, 'NODE_SEA_BLOB', join(dist, 'prove.blob'),
      '--sentinel-fuse', 'NODE_SEA_FUSE_fce680ab2cc467b6e072b8b5df1996b2',
      ...(isMac ? ['--macho-segment-name', 'NODE_SEA'] : []),
    ],
    { stdio: 'inherit' },
  );
  if (isMac) execFileSync('codesign', ['--sign', '-', exe], { stdio: 'inherit' });
  console.log('built ' + exe);
}
