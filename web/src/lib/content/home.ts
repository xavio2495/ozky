// Home page content. Edit copy here.

export const lander = {
	titleTop: 'Fully',
	titleBottom: 'Shielded',
	subhead: 'Private Payments for Every Asset, Every Wallet, Every Transfer.',
	tagline: 'Powering private stablecoin payments across Stellar and Soroban.',
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
	exploreHref: '/technology'
};

export type SolutionCard = {
	title: string;
	body: string;
	graphic: 'tetra' | 'globe' | 'halftone' | 'starburst';
};

export const solutions = {
	heading: 'Shielded Money Solutions',
	cards: [
		{
			title: 'Shielded Send',
			body: 'Move USDC, USDT, and EURC with amount, sender, and receiver hidden on-chain by default.',
			graphic: 'tetra'
		},
		{
			title: 'Selective Disclosure',
			body: 'Share scoped, revocable view keys so an auditor sees exactly what they need — nothing more.',
			graphic: 'globe'
		},
		{
			title: 'Compliance',
			body: 'In-circuit approved-set membership keeps shielded funds provably clean without revealing balances.',
			graphic: 'halftone'
		},
		{
			title: 'Research',
			body: 'Open primitives — Poseidon commitments, nullifiers, UltraHonk proofs — built in the open on Soroban.',
			graphic: 'starburst'
		}
	] as SolutionCard[]
};

export const integrates = {
	title: 'ozky integrates.',
	blurb:
		'Private rails for the money and tooling you already use — every layer of the open Stellar stack, shielded by default.',
	items: [
		{ title: 'Stellar', sub: 'Settlement Layer' },
		{ title: 'Soroban', sub: 'Smart Contracts' },
		{ title: 'USDC', sub: 'Shielded Stablecoin' },
		{ title: 'EURC', sub: 'Shielded Stablecoin' },
		{ title: 'Noir · UltraHonk', sub: 'Zero-Knowledge Prover' },
		{ title: 'Poseidon · BN254', sub: 'In-Circuit Hashing' },
		{ title: 'Freighter', sub: 'Wallet Signing' },
		{ title: 'Stellar SDK', sub: 'Transaction Layer' }
	]
};

export const cta = {
	lead: 'Ready to go',
	emphasis: 'fully shielded?',
	download: { label: 'Download ozky', href: '/' }
};
