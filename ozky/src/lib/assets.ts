// Front-end asset metadata, mirroring `core::config::ASSETS` (in-circuit asset tags
// XLM=1, USDC=2, USDT=3, EURC=4). Stellar classic assets use 7 decimals. The `balance`
// command is authoritative for *display* decimals; this table drives the asset picker and
// converts user-entered decimal amounts into the base-unit `u64` the commands expect.

export type AssetMeta = {
	code: string;
	tag: number;
	decimals: number;
	name: string;
	/** accent color (oklch/hex) for chips and the balance card. */
	accent: string;
};

export const ASSETS: AssetMeta[] = [
	{ code: 'USDC', tag: 2, decimals: 7, name: 'USD Coin', accent: 'oklch(0.62 0.17 250)' },
	{ code: 'EURC', tag: 4, decimals: 7, name: 'Euro Coin', accent: 'oklch(0.7 0.16 150)' },
	{ code: 'USDT', tag: 3, decimals: 7, name: 'Tether USD', accent: 'oklch(0.72 0.15 165)' },
	{ code: 'XLM', tag: 1, decimals: 7, name: 'Stellar Lumens', accent: 'oklch(0.75 0.04 250)' }
];

export function assetByCode(code: string): AssetMeta | undefined {
	return ASSETS.find((a) => a.code === code);
}

/**
 * Convert a user-entered decimal amount (e.g. "12.5") into base units for `decimals`.
 * Returns a JS number — safe for our ranges (≤ 2^53; 7-decimal amounts stay well under it).
 * Throws on malformed input so callers can surface a validation error.
 */
export function toBaseUnits(amount: string, decimals: number): number {
	const clean = amount.trim();
	if (!/^\d*(\.\d*)?$/.test(clean) || clean === '' || clean === '.') {
		throw new Error('Enter a valid amount');
	}
	const [whole, frac = ''] = clean.split('.');
	if (frac.length > decimals) throw new Error(`Max ${decimals} decimal places`);
	const fracPadded = (frac + '0'.repeat(decimals)).slice(0, decimals);
	const units = Number(whole || '0') * 10 ** decimals + Number(fracPadded || '0');
	if (!Number.isFinite(units) || units <= 0) throw new Error('Amount must be greater than 0');
	return units;
}
