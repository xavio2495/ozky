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
	type Channel,
	type Escrow,
	type NewAccount,
	type Payroll,
	type PublicBalance,
	type Subscription,
	type WalletStatus
} from './api';

export type Activity = {
	id: number;
	kind:
		| 'deposit'
		| 'send'
		| 'split'
		| 'payroll'
		| 'subscription'
		| 'escrow'
		| 'channel'
		| 'withdraw'
		| 'swap'
		| 'enroll'
		| 'disclose';
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
	payrolls = $state<Payroll[]>([]);
	subscriptions = $state<Subscription[]>([]);
	escrows = $state<Escrow[]>([]);
	channels = $state<Channel[]>([]);
	activity = $state<Activity[]>([]);
	loading = $state(false);
	/** Set when the pool contracts aren't configured (dev without deployed IDs). */
	notConfigured = $state(false);
	#nextId = 1;

	get activeAccount(): AccountInfo | undefined {
		return this.accounts.find((a) => a.active);
	}

	get dueCount(): number {
		return this.payrolls.filter((p) => p.due).length;
	}

	get subDueCount(): number {
		return this.subscriptions.filter((s) => s.due).length;
	}

	/** Escrows with an action waiting (releasable as payee, or a refundable contribution). */
	get escrowActionCount(): number {
		return this.escrows.filter((e) => e.releasable || e.refundable).length;
	}

	/** Channels with an action waiting (closeable as merchant, or reclaimable as subscriber). */
	get channelActionCount(): number {
		return this.channels.filter((c) => c.closeable || c.reclaimable).length;
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
		this.payrolls = [];
		this.subscriptions = [];
		this.escrows = [];
		this.channels = [];
		this.activity = [];
		await this.refreshStatus();
	}

	/** Load after unlock — accounts + shielded + public balances + payrolls + subscriptions + escrows. */
	async loadSession() {
		await this.refreshAccounts();
		await this.refreshBalances();
		await this.refreshPublicBalances();
		await this.refreshPayrolls();
		await this.refreshSubscriptions();
		await this.refreshEscrows();
		await this.refreshChannels();
		await this.refreshHistory();
	}

	async refreshPayrolls() {
		if (!this.unlocked) return;
		try {
			this.payrolls = await api.listPayrolls();
		} catch (e) {
			this.payrolls = [];
			if (!isConfigError(e)) {
				toast.error('Could not load payrolls', { description: errMessage(e) });
			}
		}
	}

	/** Run a payroll now (may be several split txs); logs activity + refreshes balances. */
	async runPayroll(id: number) {
		const p = this.payrolls.find((x) => x.id === id);
		const hashes = await runAction(
			`Running payroll${p ? ` "${p.label}"` : ''}`,
			() => api.runPayroll(id),
			{ success: (h) => `Payroll paid (${h.length} tx${h.length === 1 ? '' : 's'})` }
		);
		if (hashes && p) {
			this.log({
				kind: 'payroll',
				label: `Payroll "${p.label}"`,
				detail: `${p.payees.length} payees`,
				hash: hashes[0]
			});
		}
		await this.refreshPayrolls();
	}

	async refreshSubscriptions() {
		if (!this.unlocked) return;
		try {
			this.subscriptions = await api.listSubscriptions();
		} catch (e) {
			this.subscriptions = [];
			if (!isConfigError(e)) {
				toast.error('Could not load subscriptions', { description: errMessage(e) });
			}
		}
	}

	/** Charge a subscription now (one shielded transfer); logs activity + refreshes. */
	async runSubscription(id: number) {
		const s = this.subscriptions.find((x) => x.id === id);
		const hash = await runAction(
			`Charging subscription${s ? ` "${s.label}"` : ''}`,
			() => api.runSubscription(id),
			{ success: () => 'Subscription paid' }
		);
		if (hash && s) {
			this.log({ kind: 'subscription', label: `Subscription "${s.label}"`, hash });
		}
		await this.refreshSubscriptions();
	}

	async refreshEscrows() {
		if (!this.unlocked) return;
		try {
			this.escrows = await api.listEscrows();
		} catch (e) {
			this.escrows = [];
			if (!isConfigError(e)) {
				toast.error('Could not load escrows', { description: errMessage(e) });
			}
		}
	}

	/** Open an escrow as payee (one submit, no proof). Returns the new id; refreshes the list. */
	async openEscrow(asset: string, target: number, deadlineUnix: number, mode: string) {
		const id = await runAction(
			'Opening escrow',
			() => api.openEscrow(asset, target, deadlineUnix, mode),
			{ success: (id) => `Escrow #${id} opened`, refresh: false }
		);
		if (id !== undefined) this.log({ kind: 'escrow', label: `Opened escrow #${id}` });
		await this.refreshEscrows();
		return id;
	}

	/** Contribute to an escrow (proves + spends a note); logs activity + refreshes. */
	async contributeEscrow(escrowId: number, payeeCode: string, amount: number) {
		const idx = await runAction(
			'Contributing to escrow',
			() => api.contributeEscrow(escrowId, payeeCode, amount),
			{ success: () => 'Contribution sent' }
		);
		if (idx !== undefined) this.log({ kind: 'escrow', label: `Contributed to escrow #${escrowId}` });
		await this.refreshEscrows();
		return idx;
	}

	/** Release an escrow to the payee (proves + mints); logs activity + refreshes. */
	async releaseEscrow(escrowId: number) {
		const hash = await runAction(
			'Releasing escrow',
			() => api.releaseEscrow(escrowId),
			{ success: () => 'Escrow released' }
		);
		if (hash) this.log({ kind: 'escrow', label: `Released escrow #${escrowId}`, hash });
		await this.refreshEscrows();
	}

	/** Refund this wallet's contribution to a failed escrow (proves + mints); refreshes. */
	async refundEscrow(escrowId: number, contribIndex: number) {
		const hash = await runAction(
			'Refunding contribution',
			() => api.refundEscrow(escrowId, contribIndex),
			{ success: () => 'Contribution refunded' }
		);
		if (hash) this.log({ kind: 'escrow', label: `Refunded escrow #${escrowId}`, hash });
		await this.refreshEscrows();
	}

	async refreshChannels() {
		if (!this.unlocked) return;
		try {
			this.channels = await api.listChannels();
		} catch (e) {
			this.channels = [];
			if (!isConfigError(e)) {
				toast.error('Could not load channels', { description: errMessage(e) });
			}
		}
	}

	/** Open a subscription channel as the subscriber (proves + spends the cap). Returns the id. */
	async openChannel(
		asset: string,
		cap: number,
		merchantCode: string,
		amountPerPeriod: number,
		nPeriods: number,
		periodSecs: number
	) {
		const id = await runAction(
			'Opening channel',
			() => api.openChannel(asset, cap, merchantCode, amountPerPeriod, nPeriods, periodSecs),
			{ success: (id) => `Channel #${id} opened`, refresh: false }
		);
		if (id !== undefined) this.log({ kind: 'channel', label: `Opened channel #${id}` });
		await this.refreshChannels();
		return id;
	}

	/** Close a channel as the merchant (proves + mints both notes); logs activity + refreshes. */
	async closeChannel(channelId: number) {
		const hash = await runAction('Closing channel', () => api.closeChannel(channelId), {
			success: () => 'Channel closed'
		});
		if (hash) this.log({ kind: 'channel', label: `Closed channel #${channelId}`, hash });
		await this.refreshChannels();
	}

	/** Reclaim the full cap as the subscriber after expiry (proves + mints); refreshes. */
	async reclaimChannel(channelId: number) {
		const hash = await runAction('Reclaiming channel', () => api.reclaimChannel(channelId), {
			success: () => 'Cap reclaimed'
		});
		if (hash) this.log({ kind: 'channel', label: `Reclaimed channel #${channelId}`, hash });
		await this.refreshChannels();
	}

	/** Import a channel this wallet is the merchant for (decrypt the on-chain blob); refreshes. */
	async importChannel(channelId: number) {
		await runAction('Importing channel', () => api.importChannel(channelId), {
			success: () => `Channel #${channelId} imported`,
			refresh: false
		});
		await this.refreshChannels();
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
		// Mirror into the durable shielded-history store so it survives lock/restart (G8).
		void api
			.recordActivity(entry.kind, entry.label, entry.detail, entry.hash)
			.catch(() => {});
	}

	/** Load the durable shielded history (the wallet's own pool actions) into `activity`. */
	async refreshHistory() {
		if (!this.unlocked) return;
		try {
			const persisted = await api.shieldedHistory();
			this.activity = persisted.slice(0, 50).map((t) => ({
				id: t.id,
				kind: t.kind as Activity['kind'],
				label: t.label,
				detail: t.detail,
				hash: t.hash,
				ts: t.ts
			}));
			// Continue ids past the highest persisted one so new in-session logs don't collide.
			this.#nextId = persisted.reduce((m, t) => Math.max(m, t.id), 0) + 1;
		} catch {
			// Best-effort: an empty/locked store just leaves the in-session log.
		}
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
