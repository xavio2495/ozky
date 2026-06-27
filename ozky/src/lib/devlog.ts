// Dev aid: mirror frontend errors/warnings to the Tauri dev terminal (via the `frontend_log`
// command → Rust stderr), so UI failures — Svelte reactive-loop aborts
// (`effect_update_depth_exceeded`), render throws, unhandled promise rejections — show up in
// `npm run tauri dev` output instead of only in the webview console. No-op outside dev.
import { invoke } from '@tauri-apps/api/core';

let installed = false;

function send(level: string, message: string) {
	// Fire-and-forget; never let logging throw or recurse into the patched console.
	invoke('frontend_log', { level, message }).catch(() => {});
}

function fmt(args: unknown[]): string {
	return args
		.map((a) => {
			if (a instanceof Error) return `${a.name}: ${a.message}\n${a.stack ?? ''}`;
			if (typeof a === 'string') return a;
			try {
				return JSON.stringify(a);
			} catch {
				return String(a);
			}
		})
		.join(' ');
}

export function installDevLog() {
	if (installed || !import.meta.env.DEV || typeof window === 'undefined') return;
	installed = true;

	window.addEventListener('error', (e) => {
		send('error', `window.onerror: ${e.message} @ ${e.filename}:${e.lineno}:${e.colno}\n${e.error?.stack ?? ''}`);
	});
	window.addEventListener('unhandledrejection', (e) => {
		const r = e.reason;
		send('error', `unhandledrejection: ${r instanceof Error ? `${r.message}\n${r.stack ?? ''}` : fmt([r])}`);
	});

	for (const level of ['error', 'warn'] as const) {
		const orig = console[level].bind(console);
		console[level] = (...args: unknown[]) => {
			orig(...args);
			send(level, fmt(args));
		};
	}

	send('info', 'devlog installed — frontend errors/warnings now mirror to this terminal');
}
