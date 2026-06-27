// Local dev stack: starts the three sidecar services (funder, indexer, keeper) — and, by
// default, the Tauri app — together, with prefixed colour-coded logs and a clean Ctrl-C
// teardown. Secrets/IDs are read from the repo-root ozky.config.json so there's nothing to
// wire by hand; the local funder reuses the (already testnet-funded) relayer key as its
// source account.
//
//   npm run dev:stack       services + app   (use this to launch)
//   npm run dev:services     services only   (e.g. run the funder on its own dev server)
//
// Ports: funder 9100, indexer 9101, keeper 9102 (the app's OZKY_FUNDER_URL points at 9100).

import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { platform } from 'node:os';

const here = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(here, '..'); // ozky/
const repoRoot = resolve(appDir, '..'); // repo root (holds the sibling services + config)

let cfg = {};
try {
	cfg = JSON.parse(readFileSync(resolve(repoRoot, 'ozky.config.json'), 'utf8'));
} catch {
	console.error('dev-stack: could not read repo-root ozky.config.json');
	process.exit(1);
}

const RPC = cfg.OZKY_RPC_URL || 'https://soroban-testnet.stellar.org';
const relayer = cfg.OZKY_RELAYER_SECRET;
const pool = cfg.OZKY_POOL_CONTRACT;
const keeperToken = cfg.OZKY_KEEPER_TOKEN || 'dev-keeper-token';

if (!relayer) {
	console.error('dev-stack: ozky.config.json missing OZKY_RELAYER_SECRET (the local funder + keeper source key)');
	process.exit(1);
}

const withApp = !process.argv.includes('--no-app');

const services = [
	{
		name: 'funder',
		color: 32,
		cwd: resolve(repoRoot, 'funder-service'),
		cmd: 'cargo run',
		env: { OZKY_FUNDER_SECRET: relayer, OZKY_RPC_URL: RPC, PORT: '9100' }
	},
	{
		name: 'indexer',
		color: 36,
		cwd: resolve(repoRoot, 'indexer'),
		cmd: 'cargo run',
		env: { POOL_ID: pool ?? '', RPC_URL: RPC, PORT: '9101' }
	},
	{
		name: 'keeper',
		color: 35,
		cwd: resolve(repoRoot, 'keeper-service'),
		cmd: 'cargo run',
		env: {
			OZKY_POOL_CONTRACT: pool ?? '',
			OZKY_RELAYER_SECRET: relayer,
			OZKY_KEEPER_TOKEN: keeperToken,
			OZKY_RPC_URL: RPC,
			PORT: '9102'
		}
	}
];
if (withApp) {
	services.push({ name: 'app', color: 33, cwd: appDir, cmd: 'npm run tauri dev', env: {} });
}

if (!pool) {
	log('dev-stack', 33, 'note: OZKY_POOL_CONTRACT not set — indexer/keeper will idle/exit; funder still works.');
}

const children = [];

function log(name, color, line) {
	process.stdout.write(`\x1b[${color}m[${name}]\x1b[0m ${line}\n`);
}

for (const s of services) {
	const child = spawn(s.cmd, {
		cwd: s.cwd,
		env: { ...process.env, ...s.env },
		shell: true,
		detached: platform() !== 'win32'
	});
	children.push(child);
	const pipe = (buf) =>
		buf
			.toString()
			.split(/\r?\n/)
			.filter(Boolean)
			.forEach((l) => log(s.name, s.color, l));
	child.stdout.on('data', pipe);
	child.stderr.on('data', pipe);
	child.on('exit', (code) => log(s.name, s.color, `exited (code ${code ?? '?'})`));
	log(s.name, s.color, `starting in ${s.cwd}`);
}

function killChild(c) {
	if (!c.pid) return;
	if (platform() === 'win32') {
		try {
			spawn('taskkill', ['/pid', String(c.pid), '/T', '/F'], { stdio: 'ignore' });
		} catch {
			/* ignore */
		}
	} else {
		try {
			process.kill(-c.pid, 'SIGTERM');
		} catch {
			try {
				c.kill();
			} catch {
				/* ignore */
			}
		}
	}
}

let shuttingDown = false;
function shutdown() {
	if (shuttingDown) return;
	shuttingDown = true;
	console.log('\ndev-stack: shutting down…');
	children.forEach(killChild);
	setTimeout(() => process.exit(0), 500);
}
process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
