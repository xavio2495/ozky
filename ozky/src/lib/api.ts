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

/** A public (unshielded) balance on the wallet's classic Stellar account. */
export type PublicBalance = { code: string; balance: string; issuer: string | null };

/** A payroll payee (shielded code + base-unit amount). */
export type Payee = { code: string; amount: number };

/** A payroll as returned by the backend (+ computed `due`). */
export type Payroll = {
	id: number;
	label: string;
	asset: string;
	payees: Payee[];
	cadence: string; // "weekly" | "monthly" | "days"
	interval_days: number;
	next_run_unix: number;
	last_run_unix: number | null;
	enabled: boolean;
	due: boolean;
	total: number;
};

/** Payroll create/update input. id=0 creates. */
export type PayrollInput = {
	id: number;
	label: string;
	asset: string;
	payees: Payee[];
	cadence: string;
	interval_days: number;
	start_unix: number;
};

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

/** Result of `audit_disclosure`: a verified, read-only view of an owner's notes. */
export type AuditResult = {
	total: number;
	notes: unknown[];
};

export const api = {
	walletStatus: () => invoke<WalletStatus>('wallet_status'),
	createWallet: (password: string) => invoke<WalletSetup>('create_wallet', { password }),
	restoreWallet: (phrase: string, password: string) =>
		invoke<WalletSetup>('restore_wallet', { phrase, password }),
	unlock: (password: string, code: string) => invoke<void>('unlock', { password, code }),
	lock: () => invoke<void>('lock'),
	verifyTotp: (code: string) => invoke<boolean>('verify_totp', { code }),

	listAccounts: () => invoke<AccountInfo[]>('list_accounts'),
	createAccount: (label?: string) =>
		invoke<NewAccount>('create_account', { label: label ?? null }),
	importAccount: (phrase: string, label?: string) =>
		invoke<number>('import_account', { phrase, label: label ?? null }),
	switchAccount: (index: number) => invoke<void>('switch_account', { index }),

	publicBalances: () => invoke<PublicBalance[]>('public_balances'),
	assetPrices: () => invoke<Spot[]>('asset_prices'),
	priceHistory: (code: string, days: number) =>
		invoke<PricePoint[]>('price_history', { code, days }),

	balance: () => invoke<AssetBalance[]>('balance'),
	spendingKey: () => invoke<string>('spending_key'),
	enroll: () => invoke<string>('enroll'),

	deposit: (asset: string, amount: number) => invoke<string>('deposit', { asset, amount }),
	send: (asset: string, recipient: string, amount: number) =>
		invoke<string>('send', { asset, recipient, amount }),
	split: (asset: string, recipients: { recipient: string; amount: number }[]) =>
		invoke<string>('split', { asset, recipients }),

	listPayrolls: () => invoke<Payroll[]>('list_payrolls'),
	savePayroll: (input: PayrollInput) => invoke<number>('save_payroll', { input }),
	deletePayroll: (id: number) => invoke<void>('delete_payroll', { id }),
	setPayrollEnabled: (id: number, enabled: boolean) =>
		invoke<void>('set_payroll_enabled', { id, enabled }),
	runPayroll: (id: number) => invoke<string[]>('run_payroll', { id }),
	withdraw: (asset: string, dest: string, amount: number) =>
		invoke<string>('withdraw', { asset, dest, amount }),

	fundingAddress: () => invoke<string>('funding_address'),
	receiveAddress: () => invoke<string>('receive_address'),

	shareWithAuditor: (auditor: string, epoch: number) =>
		invoke<string>('share_with_auditor', { auditor, epoch }),
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
