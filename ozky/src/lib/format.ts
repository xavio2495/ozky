// Small display helpers shared across views.

/** Truncate a long address/code for compact display: `GABC…1234`. */
export function truncate(value: string, head = 8, tail = 6): string {
	if (!value) return '';
	if (value.length <= head + tail + 1) return value;
	return `${value.slice(0, head)}…${value.slice(-tail)}`;
}

/**
 * Prettify a base-unit-scaled decimal string (as returned by the `balance` command's
 * `display`, e.g. "1240.0000000"): group the integer part, trim trailing fraction zeros.
 */
export function prettyAmount(display: string): string {
	const [whole, frac = ''] = display.split('.');
	const grouped = (Number(whole) || 0).toLocaleString('en-US');
	const trimmed = frac.replace(/0+$/, '');
	return trimmed ? `${grouped}.${trimmed}` : grouped;
}

/** Numeric value of a `display` decimal string (for tweening). */
export function toNumber(display: string): number {
	return Number(display) || 0;
}
