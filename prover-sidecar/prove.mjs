// Docker-free UltraHonk prover (bb.js + noir_js, WASM) — sidecar.
//
// Replaces BOTH ZK-container steps that proving.rs currently shells to docker for:
//   1. `nargo execute` — solve the circuit witness from the inputs  (noir_js, WASM ACVM)
//   2. `bb prove`      — UltraHonk keccak proof                      (bb.js, WASM)
// Output is byte-for-byte the format the on-chain rs-soroban-ultrahonk verifier parses.
//
// Usage:
//   node prove.mjs <circuitDir> [frozenVkPath]
//
// Reads <circuitDir>/target/<name>.json (compiled circuit) and <circuitDir>/Prover.toml
// (the inputs the Rust core already emits via witness.rs `to_prover_toml`). Writes
// <circuitDir>/target/{proof,public_inputs} and prints a JSON report to stdout.
//
// Env:
//   BB_THREADS           1 = single-threaded (no worker threads; required by the SEA exe).
//                        Default = all CPUs.
//   OZKY_PROVER_ASSETS   dir holding the WASM blobs (bundled/SEA build). Unset in dev,
//                        where bb.js / noir_js load WASM from node_modules.

import { UltraHonkBackend } from '@aztec/bb.js';
import { Noir } from '@noir-lang/noir_js';
import { parse as parseToml } from 'smol-toml';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { cpus } from 'node:os';

async function main() {
  const circuitDir = process.argv[2];
  const frozenVkPath = process.argv[3];
  if (!circuitDir) {
    console.error('usage: node prove.mjs <circuitDir> [frozenVkPath]');
    process.exit(2);
  }
  const name = circuitDir.split(/[\\/]/).filter(Boolean).pop();
  const target = join(circuitDir, 'target');
  const threads = process.env.BB_THREADS ? Number(process.env.BB_THREADS) : Math.max(1, cpus().length);

  // In the SEA binary the WASM blobs ship beside the executable, so default to that
  // dir; an explicit OZKY_PROVER_ASSETS overrides. In dev (plain `node`) both are
  // unset and bb.js / noir_js load WASM from node_modules.
  let assetsDir = process.env.OZKY_PROVER_ASSETS;
  if (!assetsDir) {
    try {
      const sea = await import('node:sea');
      if (sea.isSea?.()) assetsDir = dirname(process.execPath);
    } catch {
      /* node:sea unavailable (old node) — dev path */
    }
  }
  const wasmPath = assetsDir ? join(assetsDir, 'barretenberg-threads.wasm.gz') : undefined;

  const compiled = JSON.parse(readFileSync(join(target, `${name}.json`), 'utf8'));
  const inputs = parseToml(readFileSync(join(circuitDir, 'Prover.toml'), 'utf8'));

  // 1. Solve the witness from inputs (replaces `nargo execute`).
  const tExec0 = Date.now();
  const { witness } = await new Noir(compiled).execute(inputs);
  const execMs = Date.now() - tExec0;

  // 2. Prove (replaces `bb prove --oracle_hash keccak`).
  const backend = new UltraHonkBackend(compiled.bytecode, { threads, wasmPath });
  const tProve0 = Date.now();
  const { proof, publicInputs } = await backend.generateProof(witness, { keccak: true });
  const proveMs = Date.now() - tProve0;

  // Public inputs → raw 32-byte big-endian fields (matches the bb CLI `public_inputs` file).
  const piBytes = Buffer.concat(
    publicInputs.map((h) => Buffer.from(h.replace(/^0x/, '').padStart(64, '0'), 'hex')),
  );

  writeFileSync(join(target, 'proof'), Buffer.from(proof));
  writeFileSync(join(target, 'public_inputs'), piBytes);

  const report = {
    circuit: name,
    threads,
    noir_version: compiled.noir_version,
    execMs,
    proveMs,
    witnessLen: witness.length,
    proofLen: proof.length,
    publicInputsLen: piBytes.length,
    numPublicInputs: publicInputs.length,
  };

  if (frozenVkPath) {
    const vk = Buffer.from(await backend.getVerificationKey({ keccak: true }));
    report.vkLen = vk.length;
    report.vkMatchesFrozen = Buffer.compare(vk, readFileSync(frozenVkPath)) === 0;
  }

  report.selfVerify = await backend.verifyProof({ proof, publicInputs }, { keccak: true });
  await backend.destroy?.();

  // Fail closed: only emit a proof that verified, and (if a frozen VK was given) whose
  // VK matches the on-chain one. Mirrors the docker path's `set -e; bb verify`.
  if (!report.selfVerify || report.vkMatchesFrozen === false) {
    console.error(JSON.stringify(report, null, 2));
    process.exit(1);
  }

  console.log(JSON.stringify(report, null, 2));
  process.exit(0);
}

main().catch((e) => {
  console.error(e?.stack || String(e));
  process.exit(1);
});
