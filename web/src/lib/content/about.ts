// About page content (merges company + contact).
export const about = {
	hero: 'Private Money for the Modern Age.',
	giant: 'About.',
	mission: {
		title: 'The default-private layer for stablecoins.',
		paras: [
			'ozky is a fully shielded stablecoin wallet on Stellar and Soroban. Notes are Poseidon commitments; spending proves Merkle membership and publishes a nullifier. Heavy cryptography runs in a native Rust core, off the UI thread.',
			"Privacy is the default — amount, sender, and receiver are hidden on-chain — while scoped, revocable view keys keep selective disclosure in the owner's hands."
		]
	},
	how: {
		title: 'Prove off-chain, verify on-chain.',
		points: [
			{
				k: 'UTXO shielded pool',
				v: 'Balances are private notes; spends prove membership and publish nullifiers.'
			},
			{
				k: 'Noir / UltraHonk',
				v: 'Proofs are generated client-side in the Rust core, off the UI thread.'
			},
			{
				k: 'Stellar host functions',
				v: 'A Soroban verifier checks proofs with BN254 pairings and Poseidon hashing.'
			},
			{
				k: 'ASP compliance',
				v: 'In-circuit approved-set membership keeps shielded funds provably clean.'
			}
		]
	},
	contact: {
		title: 'Get in touch.',
		body: "Building on private payments, or want a demo? Tell us what you're working on.",
		fields: { name: 'Name', email: 'Email', message: 'Message' },
		submit: 'Send Message'
	},
	noticeTestnet: 'ozky is testnet-first and unaudited. Independent audit before mainnet.'
};
