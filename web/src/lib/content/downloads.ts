// Downloads page content. Installers come from GitHub Releases (latest).
import { social } from './site';

export const downloads = {
	repo: 'xavio2495/ozky',
	releasesUrl: `${social.github}/releases`,
	latestReleaseUrl: `${social.github}/releases/latest`,
	latestApi: 'https://api.github.com/repos/xavio2495/ozky/releases/latest',
	sourceUrl: social.github,
	heading: 'Download ozky.',
	blurb: 'The native desktop wallet for fully shielded stablecoin payments on Stellar.',
	// Testnet honesty banner (per handoff.md — unaudited, testnet-first).
	notice: 'Testnet build — for testnet funds only. Independent audit before mainnet.',
	// Asset filename suffixes to match against the GitHub release, per platform.
	platforms: [
		{ os: 'macOS', note: 'Apple silicon & Intel · .dmg', match: ['.dmg', '.app.tar.gz'] },
		{ os: 'Windows', note: 'Windows 10 / 11 · .msi', match: ['.msi', 'setup.exe', '.exe'] },
		{ os: 'Linux', note: 'AppImage / .deb', match: ['.AppImage', '.deb'] }
	],
	requirements: [
		'64-bit macOS 12+, Windows 10/11, or a modern Linux desktop',
		'~300 MB disk, internet access to Stellar testnet',
		'No XLM needed — onboarding is relayer-funded'
	]
};
