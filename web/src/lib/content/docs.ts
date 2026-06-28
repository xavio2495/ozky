// Docs structure. Pages live under src/routes/docs/**/+page.md (mdsvex).
// The reader shell (docs/+layout.svelte) renders content on the left and a sticky
// title + numbered index on the right, and a prev/next box at the foot of each page.

export type DocLink = { label: string; href: string; level?: 0 | 1 };

// Deployed Soroban contracts (Stellar testnet). Addresses from ozky.config.json.
// The UltraHonk verifier is embedded/per-circuit (no single app-level address).
export const STELLAR_EXPERT = 'https://stellar.expert/explorer/testnet/contract';
export const contractAddresses = {
	pool: 'CCCULLPYVOFZF5WWVJNSF2HGBY3SMZWO25BPEOBXWMYM3VRM6YVVXUOF',
	policy: 'CCXRKEM3MUJBFXJOC6VMU7OUFJWSNO76LJPSTSQIODSSLZ2AMNTZG2CP',
	viewkeys: 'CDTYQIHSCRUPNGI42SLXMHHXXWMX4DQTBYMHBEXUALZ7GVZWXRI3MBSV'
} as const;

// The top-level documentation subpages (shown in the site menu).
export const docsPages: DocLink[] = [
	{ label: 'Concepts', href: '/docs/concepts' },
	{ label: 'Contracts', href: '/docs/contracts' },
	{ label: 'Circuits', href: '/docs/circuits' },
	{ label: 'Features', href: '/docs/features' },
	{ label: 'Cloud runtimes', href: '/docs/cloud' }
];

// Contracts — one page per deployed contract, under a header page.
export const contractDocs: DocLink[] = [
	{ label: 'pool', href: '/docs/contracts/pool' },
	{ label: 'policy', href: '/docs/contracts/policy' },
	{ label: 'viewkeys', href: '/docs/contracts/viewkeys' },
	{ label: 'verifier', href: '/docs/contracts/verifier' }
];

// Features — one page per feature, under a header page.
export const featureDocs: DocLink[] = [
	{ label: 'Shielded send', href: '/docs/features/shielded-send' },
	{ label: 'Deposit & withdraw', href: '/docs/features/deposit-withdraw' },
	{ label: 'Consolidate & split', href: '/docs/features/notes' },
	{ label: 'Shielded swap & pay', href: '/docs/features/swap' },
	{ label: 'Escrow', href: '/docs/features/escrow' },
	{ label: 'Payment channels', href: '/docs/features/channels' },
	{ label: 'Payroll & subscriptions', href: '/docs/features/payroll' },
	{ label: 'Auditor disclosure', href: '/docs/features/disclosure' },
	{ label: 'ASP compliance', href: '/docs/features/compliance' }
];

// Single ordered reading sequence — drives the right-rail index and prev/next.
// level 0 = section header, level 1 = sub-page.
export const docsOrder: DocLink[] = [
	{ label: 'Concepts', href: '/docs/concepts', level: 0 },
	{ label: 'Contracts', href: '/docs/contracts', level: 0 },
	...contractDocs.map((d) => ({ ...d, level: 1 as const })),
	{ label: 'Circuits', href: '/docs/circuits', level: 0 },
	{ label: 'Features', href: '/docs/features', level: 0 },
	...featureDocs.map((d) => ({ ...d, level: 1 as const })),
	{ label: 'Cloud runtimes', href: '/docs/cloud', level: 0 }
];

export function titleFor(pathname: string): string {
	return docsOrder.find((d) => d.href === pathname)?.label ?? 'Documentation';
}

// Previous / next page for the foot-of-page navigation box.
export function neighbours(pathname: string): { prev?: DocLink; next?: DocLink } {
	const i = docsOrder.findIndex((d) => d.href === pathname);
	if (i === -1) return {};
	return { prev: docsOrder[i - 1], next: docsOrder[i + 1] };
}
