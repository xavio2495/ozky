// Site-wide content: navigation, footer, and document metadata.
// Edit copy here — components read from these structures.

export type SubLink = { label: string; href: string };
export type NavItem = { label: string; href: string; children?: SubLink[] };

export const nav: NavItem[] = [
	{ label: 'Home', href: '/' },
	{ label: 'Pitch', href: '/pitch' },
	{ label: 'Download', href: '/downloads' },
	{ label: 'Quickstart', href: '/quickstart' },
	{
		label: 'Docs',
		href: '/docs',
		children: [
			{ label: 'Concepts', href: '/docs/concepts' },
			{ label: 'Contracts', href: '/docs/contracts' },
			{ label: 'Circuits', href: '/docs/circuits' },
			{ label: 'Features', href: '/docs/features' },
			{ label: 'Cloud runtimes', href: '/docs/cloud' }
		]
	},
	{ label: 'About', href: '/about' }
];

// Social / external links. More socials to be added later.
export const social = {
	github: 'https://github.com/xavio2495/ozky',
	telegram: 'https://t.me/xavio2495',
	email: '2495.immanuel@gmail.com'
} as const;

export const footer = {
	tagline: ['Shielded by default.', 'Private by design.'],
	address: ['ozky labs', 'remote-first', 'on stellar / soroban', 'testnet'],
	links: [
		{ label: 'GitHub', href: social.github },
		{ label: 'Telegram', href: social.telegram },
		{ label: 'Email', href: `mailto:${social.email}` }
	],
	legal: [
		{ label: '©2026 ozky labs' },
		{ label: 'License Agreement', href: '/legal/license' },
		{ label: 'Privacy Policy', href: '/legal/privacy' },
		{ label: 'Terms of Use', href: '/legal/terms' }
	] as { label: string; href?: string }[]
};

export const meta = {
	title: 'ozky — fully shielded stablecoin payments',
	description:
		'ozky is a fully shielded stablecoin wallet — private payments for every asset, every wallet, every transfer, on Stellar.',
	// Canonical site URL + social preview image (served from web/static/preview.png).
	// Update `url` if the production domain changes.
	url: 'https://ozky.vercel.app',
	image: 'https://ozky.vercel.app/preview.png'
};
