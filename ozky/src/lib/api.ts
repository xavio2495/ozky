// Typed wrappers over the Tauri `invoke` command surface (src-tauri/src/commands.rs).
// The UI only ever touches these — never a crypto primitive.

import { invoke } from '@tauri-apps/api/core';

export type WalletStatus = { initialized: boolean; unlocked: boolean; network: string };

/** Returned by create/restore: phrase to back up (create only) + TOTP provisioning. */
export type WalletSetup = { mnemonic: string; totp_secret: string; totp_uri: string };

/** One account (each is an independent seed — created or imported). */
export type AccountInfo = { index: number; label: string; address: string; active: boolean };

/** Result of creating a fresh account: its index + the new phrase to back up. */
export type NewAccount = { index: number; mnemonic: string };

export const MAX_ACCOUNTS = 5;

/** One account's recovery phrase, for the global "export all recovery codes" backup. */
export type RecoveryExport = { index: number; label: string; mnemonic: string };

/** A public (unshielded) balance on the wallet's classic Stellar account. */
export type PublicBalance = { code: string; balance: string; issuer: string | null };

/** A payroll payee (shielded code + base-unit amount, optional cross-asset receive). */
export type Payee = { code: string; amount: number; recv_asset?: string };

/** One funding group of a payroll: a funding asset + its payees (a tab in the composer). */
export type PayGroup = { asset: string; payees: Payee[]; total: number };

/** A payroll as returned by the backend (+ computed `due`). */
export type Payroll = {
	id: number;
	label: string;
	groups: PayGroup[];
	cadence: string; // "weekly" | "monthly" | "days"
	interval_days: number;
	next_run_unix: number;
	last_run_unix: number | null;
	end_unix: number | null;
	auditor: string | null;
	/** "auto" | "manual" (null = manual). */
	approval: string | null;
	/** "local" | "cloud" (null = local). */
	run_location: string | null;
	enabled: boolean;
	due: boolean;
	/** Cross-group base-unit sum (rough; assets may differ). */
	total: number;
	/** First group's funding asset, for compact list/calendar display. */
	primary_asset: string;
	payee_count: number;
};

/** A headless-keeper armed run summary (no proof bytes). */
export type KeeperRun = {
	payroll_id: number;
	chunks: number;
	bound_epoch: number;
	earliest_submit_unix: number;
	submitted: number;
	tx_hashes: string[];
	error: string | null;
};

/** Payroll create/update input. id=0 creates. */
export type PayrollInput = {
	id: number;
	label: string;
	groups: { asset: string; payees: Payee[] }[];
	cadence: string;
	interval_days: number;
	start_unix: number;
	/** Unix seconds to stop after; 0 = no end. */
	end_unix: number;
	/** Stellar `G…` auditor address; empty = none. */
	auditor: string;
	/** "auto" | "manual" (empty = manual). */
	approval: string;
	/** "local" | "cloud" (empty = local). */
	run_location: string;
};

/** A push subscription as returned by the backend (+ computed `due`). */
export type Subscription = {
	id: number;
	label: string;
	asset: string;
	code: string;
	amount: number;
	cadence: string; // "weekly" | "monthly" | "days"
	interval_days: number;
	next_run_unix: number;
	last_run_unix: number | null;
	end_unix: number | null;
	auditor: string | null;
	approval: string | null;
	run_location: string | null;
	enabled: boolean;
	due: boolean;
};

/** Subscription create/update input. id=0 creates; end_unix=0 means no end. */
export type SubscriptionInput = {
	id: number;
	label: string;
	asset: string;
	code: string;
	amount: number;
	cadence: string;
	interval_days: number;
	start_unix: number;
	end_unix: number;
	/** Stellar `G…` auditor address; empty = none. */
	auditor: string;
	/** "auto" | "manual" (empty = manual). */
	approval: string;
	/** "local" | "cloud" (empty = local). */
	run_location: string;
};

/** One contribution this wallet made to an escrow (for the refund affordance). */
export type EscrowContribution = { index: number; amount: number };

/** A shielded escrow this wallet opened or contributed to (+ on-chain state & eligibility). */
export type Escrow = {
	id: number;
	asset: string;
	target: number;
	mode: string; // "all_or_nothing" | "keep_what_you_raise"
	n_contrib: number;
	status: string; // "open" | "released"
	deadline_unix: number;
	deadline_passed: boolean;
	is_payee: boolean;
	my_contributions: EscrowContribution[];
	/** Payee-only decrypted running total; null for contributors. */
	raised: number | null;
	releasable: boolean;
	refundable: boolean;
};

/** A merchant-pull subscription channel this wallet opened (subscriber) or imported (merchant). */
export type Channel = {
	id: number;
	asset: string;
	status: string; // "open" | "closed"
	expiry_unix: number;
	expiry_passed: boolean;
	is_subscriber: boolean;
	is_merchant: boolean;
	/** The hidden cap (this wallet's own knowledge from the ramp). */
	cap: number;
	amount_per_period: number;
	/** Highest cumulative amount currently authorized (elapsed periods) — what a close would draw. */
	drawn_so_far: number;
	/** Merchant: a close is possible now. */
	closeable: boolean;
	/** Subscriber: a reclaim is possible now (past expiry, unclosed). */
	reclaimable: boolean;
};

/** A persisted shielded-history entry (the wallet's own pool actions). Shape matches `Activity`. */
export type ShieldedTx = {
	id: number;
	kind: string;
	label: string;
	detail?: string;
	hash?: string;
	/** Unix milliseconds. */
	ts: number;
};

/** A public (classic Stellar) payment on the wallet's funding `G…` account, from Horizon. */
export type PublicTx = {
	direction: string; // "received" | "sent"
	kind: string; // "create_account" | "payment"
	amount: string;
	asset: string;
	counterparty?: string;
	hash: string;
	/** Unix milliseconds. */
	ts: number;
};

/** An in-pool constant-product swap quote, read from the live on-chain reserves. */
export type SwapQuote = {
	/** Estimated destination amount at the current reserves, in base units. */
	dest_amount: number;
	/** Source reserve (base units). */
	reserve_from: number;
	/** Destination reserve (base units). */
	reserve_to: number;
};

/** The result of an atomic in-pool swap. */
export type SwapReceipt = {
	tx_hash: string;
	from: string;
	to: string;
	sent: number;
	received: number;
};

/** A cross-asset-pay quote: the source (X) cost to deliver the requested Y, at live reserves. */
export type PayQuote = {
	/** Estimated source (X) cost in base units (before the slippage buffer). */
	source_cost: number;
	/** Source reserve (base units). */
	reserve_from: number;
	/** Destination reserve (base units). */
	reserve_to: number;
};

/** One recipient of a multi-send: shielded code, base-unit amount, and optional receive-asset
 * (a different asset = cross-asset pay, then `amount` is the destination amount). */
export type MultiRecipient = { recipient: string; amount: number; recv_asset?: string };

/** Current USD spot price for an asset. */
export type Spot = { code: string; usd: number; change_24h: number };
/** One point on a price history series (t = unix ms). */
export type PricePoint = { t: number; usd: number };

export type AssetBalance = {
	/** v1 asset code, e.g. "USDC". */
	code: string;
	/** in-circuit asset_tag (decimal string). */
	asset_tag: string;
	/** spendable total in base units. */
	raw: number;
	/** human-readable amount (base units scaled by `decimals`). */
	display: string;
	decimals: number;
};

/** Result of `ensure_trustlines`: which sponsored trustlines were established. */
export type TrustlineReport = {
	account_created: boolean;
	added: string[];
	already: boolean;
	tx: string | null;
};

/** One disclosed note in an audit result (its opening, re-verified against the on-chain leaf). */
export type DisclosedNote = {
	leaf_index: number;
	value: number;
	asset_tag: string;
	epoch: number;
	commitment: string;
};

/** Result of `audit_disclosure`: a verified, read-only view of an owner's notes for a
 * time-bounded epoch range. */
export type AuditResult = {
	total: number;
	notes: DisclosedNote[];
	fromEpoch: number;
	toEpoch: number;
};

export const api = {
	walletStatus: () => invoke<WalletStatus>('wallet_status'),
	createWallet: (password: string) => invoke<WalletSetup>('create_wallet', { password }),
	restoreWallet: (phrase: string, password: string) =>
		invoke<WalletSetup>('restore_wallet', { phrase, password }),
	/** Confirm onboarding 2FA + commit the staged wallet. `false` ⇒ wrong code (retry). */
	finishSetup: (code: string) => invoke<boolean>('finish_setup', { code }),
	unlock: (password: string, code: string) => invoke<void>('unlock', { password, code }),
	lock: () => invoke<void>('lock'),
	logout: () => invoke<void>('logout'),
	exportRecoveryPhrases: () => invoke<RecoveryExport[]>('export_recovery_phrases'),
	verifyTotp: (code: string) => invoke<boolean>('verify_totp', { code }),

	listAccounts: () => invoke<AccountInfo[]>('list_accounts'),
	createAccount: (label?: string) =>
		invoke<NewAccount>('create_account', { label: label ?? null }),
	importAccount: (phrase: string, label?: string) =>
		invoke<number>('import_account', { phrase, label: label ?? null }),
	switchAccount: (index: number) => invoke<void>('switch_account', { index }),
	renameAccount: (index: number, label: string) =>
		invoke<void>('rename_account', { index, label }),

	publicBalances: () => invoke<PublicBalance[]>('public_balances'),
	assetPrices: () => invoke<Spot[]>('asset_prices'),
	priceHistory: (code: string, days: number) =>
		invoke<PricePoint[]>('price_history', { code, days }),

	balance: () => invoke<AssetBalance[]>('balance'),
	spendingKey: () => invoke<string>('spending_key'),
	enroll: () => invoke<string>('enroll'),

	publicHistory: () => invoke<PublicTx[]>('public_history'),
	shieldedHistory: () => invoke<ShieldedTx[]>('shielded_history'),
	recordActivity: (kind: string, label: string, detail?: string, hash?: string) =>
		invoke<ShieldedTx>('record_activity', {
			kind,
			label,
			detail: detail ?? null,
			hash: hash ?? null
		}),

	deposit: (asset: string, amount: number) => invoke<string>('deposit', { asset, amount }),
	send: (asset: string, recipient: string, amount: number) =>
		invoke<string>('send', { asset, recipient, amount }),
	publicSend: (asset: string, dest: string, amount: number) =>
		invoke<string>('public_send', { asset, dest, amount }),
	publicToShielded: (asset: string, recipient: string, amount: number) =>
		invoke<string>('public_to_shielded', { asset, recipient, amount }),
	consolidate: (asset: string) => invoke<string>('consolidate', { asset }),
	split: (asset: string, recipients: { recipient: string; amount: number }[]) =>
		invoke<string>('split', { asset, recipients }),

	listPayrolls: () => invoke<Payroll[]>('list_payrolls'),
	savePayroll: (input: PayrollInput) => invoke<number>('save_payroll', { input }),
	deletePayroll: (id: number) => invoke<void>('delete_payroll', { id }),
	setPayrollEnabled: (id: number, enabled: boolean) =>
		invoke<void>('set_payroll_enabled', { id, enabled }),
	runPayroll: (id: number) => invoke<string[]>('run_payroll', { id }),

	armPayrollKeeper: (id: number) => invoke<KeeperRun>('arm_payroll_keeper', { id }),
	disarmPayrollKeeper: (id: number) => invoke<boolean>('disarm_payroll_keeper', { id }),
	keeperStatus: () => invoke<KeeperRun[]>('keeper_status'),
	keeperEndpoint: () => invoke<string>('keeper_endpoint'),
	setKeeperEndpoint: (url: string, token: string) =>
		invoke<void>('set_keeper_endpoint', { url, token }),
	setLocalKeeper: (enabled: boolean) => invoke<boolean>('set_local_keeper', { enabled }),
	localKeeperStatus: () => invoke<boolean>('local_keeper_status'),

	listSubscriptions: () => invoke<Subscription[]>('list_subscriptions'),
	saveSubscription: (input: SubscriptionInput) => invoke<number>('save_subscription', { input }),
	deleteSubscription: (id: number) => invoke<void>('delete_subscription', { id }),
	setSubscriptionEnabled: (id: number, enabled: boolean) =>
		invoke<void>('set_subscription_enabled', { id, enabled }),
	runSubscription: (id: number) => invoke<string>('run_subscription', { id }),

	listEscrows: () => invoke<Escrow[]>('list_escrows'),
	openEscrow: (asset: string, target: number, deadlineUnix: number, mode: string) =>
		invoke<number>('open_escrow', { asset, target, deadlineUnix, mode }),
	contributeEscrow: (escrowId: number, payeeCode: string, amount: number) =>
		invoke<number>('contribute_escrow', { escrowId, payeeCode, amount }),
	releaseEscrow: (escrowId: number) => invoke<string>('release_escrow', { escrowId }),
	refundEscrow: (escrowId: number, contribIndex: number) =>
		invoke<string>('refund_escrow', { escrowId, contribIndex }),

	listChannels: () => invoke<Channel[]>('list_channels'),
	openChannel: (
		asset: string,
		cap: number,
		merchantCode: string,
		amountPerPeriod: number,
		nPeriods: number,
		periodSecs: number
	) =>
		invoke<number>('open_channel', {
			asset,
			cap,
			merchantCode,
			amountPerPeriod,
			nPeriods,
			periodSecs
		}),
	closeChannel: (channelId: number) => invoke<string>('close_channel', { channelId }),
	reclaimChannel: (channelId: number) => invoke<string>('reclaim_channel', { channelId }),
	importChannel: (channelId: number) => invoke<void>('import_channel', { channelId }),

	withdraw: (asset: string, dest: string, amount: number) =>
		invoke<string>('withdraw', { asset, dest, amount }),

	swapQuote: (from: string, to: string, amount: number) =>
		invoke<SwapQuote>('swap_quote', { from, to, amount }),
	swap: (from: string, to: string, amount: number, slippageBps: number) =>
		invoke<SwapReceipt>('swap', { from, to, amount, slippageBps }),

	payQuote: (from: string, to: string, destAmount: number) =>
		invoke<PayQuote>('pay_quote', { from, to, destAmount }),
	pay: (
		recipientCode: string,
		from: string,
		to: string,
		destAmount: number,
		slippageBps: number
	) => invoke<SwapReceipt>('pay', { recipientCode, from, to, destAmount, slippageBps }),
	multiSend: (payAsset: string, recipients: MultiRecipient[]) =>
		invoke<string[]>('multi_send', { payAsset, recipients }),

	ensureTrustlines: () => invoke<TrustlineReport>('ensure_trustlines'),
	/** Onboarding: fund the new account via the server funder (10 XLM) + add trustlines locally. */
	provisionAccount: () => invoke<TrustlineReport>('provision_account'),

	fundingAddress: () => invoke<string>('funding_address'),
	receiveAddress: () => invoke<string>('receive_address'),

	shareWithAuditor: (auditor: string, fromEpoch: number, toEpoch: number) =>
		invoke<string>('share_with_auditor', { auditor, fromEpoch, toEpoch }),
	auditDisclosure: (pkg: string) =>
		invoke<string>('audit_disclosure', { package: pkg }).then(
			(s) => JSON.parse(s) as AuditResult
		)
};

/** Normalize an invoke error for display. CoreError serializes to `{kind, message}`
 * (or `{kind}` for unit variants like `Locked`/`NoWallet`). */
export function errMessage(e: unknown): string {
	if (typeof e === 'string') return e;
	if (e && typeof e === 'object') {
		const o = e as { message?: unknown; kind?: unknown };
		if (typeof o.message === 'string') return o.message;
		if (typeof o.kind === 'string') return o.kind;
	}
	if (e instanceof Error) return e.message;
	return String(e);
}

/** Whether an invoke error is the "pool/contracts not configured" case (dev without
 * deployed contract IDs) rather than a real failure. */
export function isConfigError(e: unknown): boolean {
	const m = errMessage(e);
	return m.includes('OZKY_') || m.includes('not set') || m.includes('not configured');
}
