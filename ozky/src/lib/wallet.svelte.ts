// Global, reactive wallet state (Svelte 5 runes). Shared by the shell and all views so a
// single status/balance scan is reused. Also an in-session activity log (there is no
// backend history command yet) and a toast-wrapped action runner.

import { toast } from 'svelte-sonner';
import {
	api,
	errMessage,
	isConfigError,
	type AccountInfo,
	type AssetBalance,
	type NewAccount,
	type PublicBalance,
	type WalletStatus
} from './api';

export type Activity = {
	id: number;
	kind: 'deposit' | 'send' | 'withdraw' | 'enroll' | 'disclose';
	label: string;
	detail?: string;
	hash?: string;
	ts: number;
};

class WalletStore {
	status = $state<WalletStatus | null>(null);
	balances = $state<AssetBalance[]>([]);
	publicBalances = $state<PublicBalance[]>([]);
	accounts = $state<AccountInfo[]>([]);
	activity = $state<Activity[]>([]);
	loading = $state(false);
	/** Set when the pool contracts aren't configured (dev without deployed IDs). */
	notConfigured = $state(false);
	#nextId = 1;

	get activeAccount(): AccountInfo | undefined {
		return this.accounts.find((a) => a.active);
	}

	get initialized() {
		return this.status?.initialized ?? false;
	}
	get unlocked() {
		return this.status?.unlocked ?? false;
	}
	get network() {
		return this.status?.network ?? 'testnet';
	}

	async lock() {
		await api.lock();
		this.balances = [];
		this.publicBalances = [];
		this.accounts = [];
		this.activity = [];
		await this.refreshStatus();
	}

	/** Load after unlock — the accounts + shielded + public balances for the active one. */
	async loadSession() {
		await this.refreshAccounts();
		await this.refreshBalances();
		await this.refreshPublicBalances();
	}

	/** The active account's public (unshielded) Stellar balances — independent of the pool. */
	async refreshPublicBalances() {
		if (!this.unlocked) return;
		try {
			this.publicBalances = await api.publicBalances();
		} catch (e) {
			this.publicBalances = [];
			toast.error('Could not load public balance', { description: errMessage(e) });
		}
	}

	async refreshAccounts() {
		if (!this.unlocked) return;
		try {
			this.accounts = await api.listAccounts();
		} catch (e) {
			toast.error('Could not load accounts', { description: errMessage(e) });
		}
	}

	/** Switch the active account, then reload its accounts list + balances + activity. */
	async switchAccount(index: number) {
		await api.switchAccount(index);
		this.activity = [];
		await this.refreshAccounts();
		await this.refreshBalances();
		await this.refreshPublicBalances();
	}

	/** Create a brand-new account (own fresh seed, max 5) and switch to it. Returns the
	 * new account incl. its recovery phrase, which the caller must show once. */
	async createAccount(label?: string): Promise<NewAccount> {
		const created = await api.createAccount(label);
		this.activity = [];
		await this.refreshAccounts();
		await this.refreshBalances();
		await this.refreshPublicBalances();
		return created;
	}

	/** Import an existing wallet by recovery phrase (max 5) and switch to it. */
	async importAccount(phrase: string, label?: string) {
		await api.importAccount(phrase, label);
		this.activity = [];
		await this.refreshAccounts();
		await this.refreshBalances();
		await this.refreshPublicBalances();
	}

	async refreshStatus() {
		this.status = await api.walletStatus();
	}

	async refreshBalances() {
		if (!this.unlocked) return;
		this.loading = true;
		try {
			this.balances = await api.balance();
			this.notConfigured = false;
		} catch (e) {
			this.balances = [];
			// Missing pool config in dev is expected — degrade calmly, don't alarm.
			if (isConfigError(e)) {
				this.notConfigured = true;
			} else {
				toast.error('Could not load balance', { description: errMessage(e) });
			}
		} finally {
			this.loading = false;
		}
	}

	log(entry: Omit<Activity, 'id' | 'ts'>) {
		this.activity.unshift({ ...entry, id: this.#nextId++, ts: Date.now() });
		this.activity = this.activity.slice(0, 50);
	}
}

export const wallet = new WalletStore();

/**
 * Run a long action (deposit/send/withdraw/enroll/disclose) with a single managed toast and
 * refresh balances on success. `onHash` receives the returned tx hash so the caller can show
 * it / push richer activity.
 */
export async function runAction<T>(
	pending: string,
	fn: () => Promise<T>,
	opts: { success?: (r: T) => string; refresh?: boolean } = {}
): Promise<T | undefined> {
	const id = toast.loading(pending);
	try {
		const result = await fn();
		toast.success(opts.success ? opts.success(result) : 'Done', { id });
		if (opts.refresh !== false) await wallet.refreshBalances();
		return result;
	} catch (e) {
		toast.error('Action failed', { id, description: errMessage(e) });
		return undefined;
	}
}
