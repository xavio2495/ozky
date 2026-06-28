// Quickstart page content — zero to first shielded payment.
type Step = { title: string; body: string; href?: string; hrefLabel?: string };

export const quickstart: {
	heading: string;
	blurb: string;
	steps: Step[];
	footerNote: string;
} = {
	heading: 'Quickstart.',
	blurb: 'From install to your first fully shielded payment in about five minutes.',
	steps: [
		{
			title: 'Download & install',
			body: 'Grab the desktop app for macOS, Windows, or Linux and open it.',
			href: '/downloads',
			hrefLabel: 'Go to downloads ↗'
		},
		{
			title: 'Create your wallet',
			body: 'Generate a 12-word recovery phrase and store it safely. It derives both your Stellar key and your ZK spending + view keys.'
		},
		{
			title: 'Get funded automatically',
			body: 'Onboarding is relayer-funded: trustlines for USDC, USDT, and EURC are provisioned for you. You never touch a public XLM account.'
		},
		{
			title: 'Deposit into the shield',
			body: 'Move testnet stablecoins from public balance into the shielded pool. They become private notes — Poseidon commitments only you can spend.'
		},
		{
			title: 'Send your first shielded payment',
			body: 'Paste a recipient, pick an amount, and send. Amount, sender, and receiver are hidden on-chain; change returns to you automatically.'
		},
		{
			title: 'Share a scoped statement (optional)',
			body: 'Hand an auditor a revocable view key for one account, asset, and epoch — they verify against the chain and see nothing else.'
		}
	],
	footerNote: 'Testnet only — for testnet funds. Independent audit before mainnet.'
};
