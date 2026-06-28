// Legal pages content — License Agreement, Privacy Policy, Terms of Use.
// Plain, accurate copy for a non-custodial, testnet, GPL-3.0 desktop wallet.
// Each page = a title, an effective date, and an ordered list of sections.

import { social } from './site';

export type LegalSection = { h: string; p: string[] };
export type LegalDoc = {
	slug: string;
	label: string; // index / nav label
	title: string; // giant page title
	updated: string;
	intro: string;
	sections: LegalSection[];
};

const REPO = 'https://github.com/xavio2495/ozky';

export const license: LegalDoc = {
	slug: 'license',
	label: 'License Agreement',
	title: 'License.',
	updated: '28 June 2026',
	intro:
		'ozky is free and open-source software, licensed under the GNU General Public License, version 3.0 (GPL-3.0). You may use, study, share, and modify it under the terms below.',
	sections: [
		{
			h: 'GNU GPL v3.0',
			p: [
				'The ozky wallet, its Rust core, the Soroban contracts, and the Noir circuits are released under the GPL-3.0. You are free to run the program for any purpose, study how it works, redistribute copies, and distribute modified versions.',
				`The complete, authoritative license text ships with the source as the LICENSE file: ${REPO}/blob/main/LICENSE.`
			]
		},
		{
			h: 'Copyleft',
			p: [
				'If you distribute the software or a derivative work — modified or not — you must pass on the same GPL-3.0 freedoms to your recipients and make the corresponding source available. You may not impose further restrictions on the rights granted by the license.'
			]
		},
		{
			h: 'No warranty',
			p: [
				'As stated in sections 15 and 16 of the GPL-3.0, the program is provided "as is", without warranty of any kind, to the extent permitted by applicable law. ozky is testnet-first and has not been independently audited; an audit is required before any mainnet use.'
			]
		},
		{
			h: 'Trademarks',
			p: [
				'The GPL-3.0 grants rights to the software, not to the "ozky" name or logo. The wordmark and brand assets are not licensed for use in a way that implies endorsement or affiliation.'
			]
		}
	]
};

export const privacy: LegalDoc = {
	slug: 'privacy',
	label: 'Privacy Policy',
	title: 'Privacy.',
	updated: '28 June 2026',
	intro:
		'Privacy is the product. ozky is a non-custodial, local-first desktop wallet: your keys, notes, and balances never leave your device, and we never see them.',
	sections: [
		{
			h: 'The wallet',
			p: [
				'ozky runs entirely on your machine. Your 12-word recovery phrase, the ZK keys derived from it, and your shielded notes are generated and stored locally. They are never transmitted to us or to any third party.',
				'On-chain, ozky is shielded by default: amount, sender, and receiver are hidden. Proving and note scanning happen in the native Rust core on your device — not on a server.'
			]
		},
		{
			h: 'Cloud runtimes',
			p: [
				'Optional services (relayer, funder, keeper, indexer) exist to abstract fees, onboard accounts, run scheduled payroll, and speed up scanning. Where used, they handle only what is needed to broadcast or index public, already-shielded transactions. They never receive your spending key, your viewing keys, or your note plaintext.'
			]
		},
		{
			h: 'This website',
			p: [
				'This marketing site is a static SvelteKit app deployed to Vercel. It sets no advertising or cross-site tracking cookies. Vercel may collect standard, aggregated request logs (such as IP address and user agent) as part of serving the site.'
			]
		},
		{
			h: 'Contact',
			p: [
				`The contact form opens your own mail client to send a message to ${social.email}; nothing is submitted to a server on this site. If you email us, we use your message only to reply.`
			]
		}
	]
};

export const terms: LegalDoc = {
	slug: 'terms',
	label: 'Terms of Use',
	title: 'Terms.',
	updated: '28 June 2026',
	intro:
		'By downloading or using ozky you agree to these terms. ozky is experimental, testnet-first software provided without warranty.',
	sections: [
		{
			h: 'Testnet & unaudited',
			p: [
				'ozky currently targets test networks and has not been independently audited. Do not use it to secure assets of real value. Mainnet support depends on the completion of a security audit. Funds and transactions on testnet have no monetary value.'
			]
		},
		{
			h: 'Non-custodial',
			p: [
				'You are solely responsible for safeguarding your recovery phrase and keys. We cannot access, freeze, recover, or reverse your wallet or any transaction. If you lose your recovery phrase, your funds are unrecoverable. There is no password reset and no support backdoor.'
			]
		},
		{
			h: 'No warranty & liability',
			p: [
				'The software is provided "as is" and "as available", without warranties of any kind, consistent with the GPL-3.0. To the maximum extent permitted by law, the authors and contributors are not liable for any loss arising from your use of, or inability to use, the software.'
			]
		},
		{
			h: 'Compliance',
			p: [
				'You are responsible for using ozky in accordance with the laws and regulations that apply to you, including any tax and reporting obligations. ozky provides scoped, revocable view keys and ASP approved-set membership so you can disclose and demonstrate compliance when you choose to.'
			]
		}
	]
};

export const legalDocs: LegalDoc[] = [license, privacy, terms];
