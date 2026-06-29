// /connect — service-discovery broker between the ozky desktop app and the GCP
// (Cloud Run) backend services. The app does NOT hardcode the service URLs; it asks
// this endpoint, which reads them from Vercel env vars and live-probes each /health so
// the app knows which servers are actually reachable. If a server is missing/down the
// app surfaces a "service unavailable — contact the developer" popup.
//
// Env vars (set in the Vercel project — see SERVICE_URLS.md at repo root):
//   OZKY_FUNDER_URL, OZKY_INDEXER_URL, OZKY_KEEPER_URL  (Cloud Run https URLs)
//
// This route is dynamic (runtime env + live probe), so it opts out of the site-wide
// prerender (see ../+layout.ts).
import { json } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';
import type { RequestHandler } from './$types';

export const prerender = false;

const SERVICES = [
	['funder', 'OZKY_FUNDER_URL'],
	['indexer', 'OZKY_INDEXER_URL'],
	['keeper', 'OZKY_KEEPER_URL']
] as const;

/** GET <url>/health with a short timeout; true on a 2xx, false on anything else. */
async function probe(url: string): Promise<boolean> {
	try {
		const res = await fetch(url.replace(/\/+$/, '') + '/health', {
			signal: AbortSignal.timeout(5000)
		});
		return res.ok;
	} catch {
		return false;
	}
}

const CORS = {
	'cache-control': 'no-store',
	'access-control-allow-origin': '*'
};

const handler: RequestHandler = async () => {
	const services: Record<string, { url: string | null; up: boolean }> = {};
	await Promise.all(
		SERVICES.map(async ([name, key]) => {
			const url = env[key]?.trim() || null;
			services[name] = { url, up: url ? await probe(url) : false };
		})
	);
	// `reachable` = at least one configured service answered; lets the app distinguish
	// "broker reached, backends are configured" from a totally unconfigured deployment.
	const reachable = Object.values(services).some((s) => s.up);
	return json({ ok: true, reachable, services }, { headers: CORS });
};

// The app POSTs to discover (per the connect flow); GET is supported for manual checks.
export const GET = handler;
export const POST = handler;
