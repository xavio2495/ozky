// Downloads page content. Installers come from GitHub Releases (latest).
import { social } from './site';

export type FileType = { label: string; match: string };
export type Platform = { os: 'macOS' | 'Windows' | 'Linux'; note: string; types: FileType[] };

export const downloads = {
	repo: 'xavio2495/ozky',
	releasesUrl: `${social.github}/releases`,
	latestReleaseUrl: `${social.github}/releases/latest`,
	latestApi: 'https://api.github.com/repos/xavio2495/ozky/releases/latest',
	sourceUrl: social.github,
	heading: 'Download.',
	// Testnet honesty banner (per handoff.md — unaudited, testnet-first).
	notice: 'Testnet build — for testnet funds only. Independent audit before mainnet.',
	// Per-OS file types. The first type that resolves to a release asset is the default.
	platforms: [
		{
			os: 'macOS',
			note: 'Apple silicon & Intel',
			types: [
				{ label: '.dmg — disk image', match: '.dmg' },
				{ label: '.app.tar.gz — archive', match: '.app.tar.gz' }
			]
		},
		{
			os: 'Windows',
			note: 'Windows 10 / 11',
			types: [
				{ label: '.msi — installer', match: '.msi' },
				{ label: '.exe — setup', match: '.exe' }
			]
		},
		{
			os: 'Linux',
			note: 'Modern desktop',
			types: [
				{ label: '.AppImage — portable', match: '.AppImage' },
				{ label: '.deb — Debian / Ubuntu', match: '.deb' }
			]
		}
	] as Platform[],
	requirements: [
		'64-bit macOS 12+, Windows 10/11, or a modern Linux desktop',
		'~300 MB disk, internet access to Stellar testnet',
		'No XLM needed — onboarding is relayer-funded'
	]
};
