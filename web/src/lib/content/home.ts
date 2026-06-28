// Home page content. Edit copy here.

export const lander = {
	titleTop: 'Fully',
	titleBottom: 'Shielded',
	subhead: 'Powering private stablecoin payments on Stellar',
	tagline: '',
	status: [
		'Poseidon Commitments',
		'Nullifiers',
		'Merkle Tree',
		'View Tags',
		'UltraHonk Prover',
		'Note Scanner',
		'ASP Compliance',
		'Scoped View Keys',
		'BN254 Curve',
		'Stellar Settlement'
	],
	feats: [
		{
			tag: 'Protocol / 2026',
			title: 'Fully Shielded Stablecoin Payments Arrive on Stellar',
			body: 'Client-side UltraHonk proofs keep amount, sender, and receiver private end to end.'
		},
		{
			tag: 'Privacy / 2026',
			title: 'Selective Disclosure Lands With Scoped View Keys',
			body: 'Share a revocable, read-only view of your activity with an auditor — nothing more.'
		},
		{
			tag: 'Compliance / 2026',
			title: 'ASP Approved-Set Membership Built Into the Circuit',
			body: 'Prove good standing without revealing identity, balances, or counterparties.'
		}
	],
	exploreLabel: 'Explore the platform.',
	exploreHref: '/downloads'
};

export type SolutionCard = {
	title: string;
	body: string;
	href: string;
	graphic: 'tetra' | 'globe' | 'halftone' | 'starburst';
};

export const solutions = {
	heading: 'Zero-knowledge money — for the jobs a wallet actually does.',
	cards: [
		{
			title: 'Pay many at once',
			body: 'One shielded transfer fans out to many recipients — split a bill, pay a team — amounts and parties hidden on-chain.',
			href: '/docs/features/notes',
			graphic: 'tetra'
		},
		{
			title: 'Run payroll',
			body: 'Scheduled shielded payouts over a saved list, run locally or by a cloud keeper that never holds a spending key.',
			href: '/docs/features/payroll',
			graphic: 'globe'
		},
		{
			title: 'Show your books',
			body: 'Hand an auditor a scoped, revocable view key — they verify against the chain and see nothing outside the scope.',
			href: '/docs/features/disclosure',
			graphic: 'halftone'
		},
		{
			title: 'Escrow & group pay',
			body: 'Many payers fund one payee privately, with a guaranteed refund on expiry — the honest substitute for a "pull".',
			href: '/docs/features/escrow',
			graphic: 'starburst'
		},
		{
			title: 'Swap in private',
			body: 'Move between stablecoins on an in-pool shielded AMM — one atomic transaction, no public DEX edge.',
			href: '/docs/features/swap',
			graphic: 'tetra'
		},
		{
			title: 'Stay compliant',
			body: 'Every spend proves in-circuit that funds trace to an approved set — provably clean, with the graph still private.',
			href: '/docs/features/compliance',
			graphic: 'globe'
		}
	] as SolutionCard[]
};

export type FeatureTile = { title: string; sub: string; href: string };

export const integrates = {
	title: 'Every feature, end to end.',
	blurb:
		'Each capability the wallet ships today — open the docs to see exactly how it works on Stellar and Soroban.',
	items: [
		{
			title: 'Shielded send',
			sub: 'Hidden amount, sender, receiver',
			href: '/docs/features/shielded-send'
		},
		{
			title: 'Deposit & withdraw',
			sub: 'Shield / unshield at the edge',
			href: '/docs/features/deposit-withdraw'
		},
		{ title: 'Consolidate & split', sub: 'Manage notes, pay many', href: '/docs/features/notes' },
		{ title: 'Shielded swap', sub: 'In-pool shielded AMM', href: '/docs/features/swap' },
		{ title: 'Escrow', sub: 'Contribute-then-payout', href: '/docs/features/escrow' },
		{
			title: 'Payment channels',
			sub: 'Merchant-pull, one settlement',
			href: '/docs/features/channels'
		},
		{
			title: 'Payroll & subscriptions',
			sub: 'Scheduled shielded payouts',
			href: '/docs/features/payroll'
		},
		{
			title: 'Auditor disclosure',
			sub: 'Scoped, revocable view keys',
			href: '/docs/features/disclosure'
		},
		{
			title: 'ASP compliance',
			sub: 'Provably clean, still private',
			href: '/docs/features/compliance'
		},
		{ title: 'All features', sub: 'Read the docs ↗', href: '/docs/features' }
	] as FeatureTile[]
};

export const cta = {
	lead: 'Ready to go',
	emphasis: 'fully shielded?',
	download: { label: 'Download ozky', href: '/downloads' }
};
