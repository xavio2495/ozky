// Client-side wallet preferences (FEATURE_SET G7 / build_plan A5). These are pure UX/strategy
// settings — NEVER an on-chain flag. The privacy mode drives client-side timing/sizing only, so
// transactions from either mode are indistinguishable on-chain (handoff decision 3, docs §7).

import { browser } from '$app/environment';

/** "instant" submits immediately; "max" adds a randomized client-side delay before submitting an
 *  interior payment, decorrelating the on-chain submission time from your action ("may take a few
 *  minutes"). Both produce identical-looking on-chain commitments/nullifiers. */
export type PrivacyMode = 'instant' | 'max';

const KEY = 'ozky.privacyMode';

/** Max-privacy timing-jitter window (ms). The delay is drawn uniformly from [min, max] so the
 *  submission time carries no signal. Kept modest but honestly surfaced as "up to a few minutes". */
export const PRIVACY_DELAY_MIN_MS = 20_000;
export const PRIVACY_DELAY_MAX_MS = 150_000;

function load(): PrivacyMode {
	if (!browser) return 'instant';
	return localStorage.getItem(KEY) === 'max' ? 'max' : 'instant';
}

class Settings {
	#privacyMode = $state<PrivacyMode>(load());

	get privacyMode(): PrivacyMode {
		return this.#privacyMode;
	}
	set privacyMode(v: PrivacyMode) {
		this.#privacyMode = v;
		if (browser) localStorage.setItem(KEY, v);
	}

	/** A randomized jitter (ms) for the given mode: 0 for instant, uniform in the window for max. */
	privacyDelayMs(mode: PrivacyMode = this.#privacyMode): number {
		if (mode !== 'max') return 0;
		const span = PRIVACY_DELAY_MAX_MS - PRIVACY_DELAY_MIN_MS;
		return PRIVACY_DELAY_MIN_MS + Math.floor(Math.random() * span);
	}
}

export const settings = new Settings();

/** Standard denomination quick-picks for the public edges (deposit/withdraw), where amounts ARE
 *  visible on-chain. Sending round, common sizes blends your public amount into everyone else's —
 *  the "sizing" half of the denomination policy. */
export const STANDARD_DENOMINATIONS = [1, 5, 10, 25, 50, 100, 500, 1000];
