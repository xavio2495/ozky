// Pitch deck content — horizontal slides for investors and users alike.
// Focused on the zero-knowledge core of ozky. Copy is kept short so each slide
// fits one viewport; `bg` drives the panel color and header logo tone.

export type PitchSlide = {
	n: string; // 01–NN
	kicker: string; // mono eyebrow
	title: string; // display headline
	body: string; // supporting paragraph (1–2 sentences)
	points?: string[]; // optional bullets (≤3, short)
	bg: 'ink' | 'gold' | 'grey';
	cta?: { label: string; href: string }[];
};

export const pitch: PitchSlide[] = [
	{
		n: '01',
		kicker: 'ozky — the pitch',
		title: 'Zero-knowledge money on Stellar.',
		body: 'ozky turns every stablecoin payment into a zero-knowledge proof — amount, sender, and receiver hidden on-chain, yet provable on demand.',
		bg: 'ink'
	},
	{
		n: '02',
		kicker: 'The problem',
		title: 'Public ledgers leak everything.',
		body: 'Every USDC transfer writes the amount, both parties, and the timing to a ledger anyone can read and correlate forever.',
		points: [
			'Addresses cluster — one linked payment leaks the rest.',
			'For payroll and treasury, that exposure is a liability.'
		],
		bg: 'gold'
	},
	{
		n: '03',
		kicker: 'The primitive',
		title: 'Balances become commitments.',
		body: 'Funds live as private notes — Poseidon commitments in an append-only Merkle tree (depth 20). You hold secrets, not ledger entries.',
		points: [
			'No account, no public balance.',
			'A view tag lets only you scan and find your notes.'
		],
		bg: 'grey'
	},
	{
		n: '04',
		kicker: 'The proof',
		title: 'Spend by proving, not revealing.',
		body: 'To spend, you prove Merkle membership in zero-knowledge and publish a nullifier — Poseidon(rho, owner_sk).',
		points: [
			'The nullifier kills double-spends without naming the note.',
			'Value conserves in-circuit; amounts stay hidden.'
		],
		bg: 'ink'
	},
	{
		n: '05',
		kicker: 'The stack',
		title: 'Noir + UltraHonk, verified on-chain.',
		body: 'Proofs are generated client-side in a native Rust core, then verified by Soroban using Stellar’s BN254 and Poseidon host functions.',
		points: [
			'Proving and scanning run off the UI thread.',
			'A native ozky-prover sidecar — no Docker for end users.'
		],
		bg: 'gold'
	},
	{
		n: '06',
		kicker: 'The circuits',
		title: 'Nine circuits, one shielded pool.',
		body: 'Every action is its own UltraHonk circuit, all settling into one atomic pool — no public DEX edge, no bridge.',
		points: [
			'deposit · transfer4 · withdraw · split.',
			'shielded swap (x·y=k) · escrow · payment channels.'
		],
		bg: 'grey'
	},
	{
		n: '07',
		kicker: 'Compliance, in-circuit',
		title: 'Clean by proof. Private by default.',
		body: 'Each spend also proves approved-set membership — owner_pk ∈ asp_root — so shielded funds stay provably clean.',
		points: [
			'Scoped view keys disclose a slice, never the spending key.',
			'You prove what you choose, to whom you choose; revocable.'
		],
		bg: 'ink'
	},
	{
		n: '08',
		kicker: 'Traction',
		title: 'Live zero-knowledge on testnet.',
		body: 'Three Soroban contracts and nine UltraHonk circuits are deployed and exercised on Stellar testnet — real proofs verified on-chain.',
		points: [
			'transfer4: 13 public inputs, verified on-chain.',
			'In-pool swap: 14 PI, value conservation exact.'
		],
		bg: 'gold'
	},
	{
		n: '09',
		kicker: 'The ask',
		title: 'Audit the circuits. Then mainnet.',
		body: 'ozky is testnet-first and unaudited by design — a circuit audit is the hard gate to mainnet. We’re looking for auditors, partners, and early users.',
		cta: [
			{ label: 'Download ozky ↗', href: '/downloads' },
			{ label: 'Get in touch ↗', href: '/about' }
		],
		bg: 'ink'
	}
];
