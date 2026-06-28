import { redirect } from '@sveltejs/kit';

// /docs has no page of its own — land on the first subpage.
export function load() {
	redirect(307, '/docs/concepts');
}
