<script lang="ts">
	import { onMount } from 'svelte';
	import { get } from 'svelte/store';
	import { page } from '$app/stores';
	import { fly } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import * as Tabs from '$lib/components/ui/tabs';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Empty from '$lib/components/ui/empty';
	import * as Popover from '$lib/components/ui/popover';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { Calendar } from '$lib/components/ui/calendar';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import CalendarMonth, { type CalEvent } from '$lib/components/ui-kit/CalendarMonth.svelte';
	import Qr from '$lib/components/shared/Qr.svelte';
	import AccountAvatar from '$lib/components/nav/AccountAvatar.svelte';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import CalendarIcon from '@lucide/svelte/icons/calendar';
	import { CalendarDate, type DateValue, getLocalTimeZone, today as cdToday } from '@internationalized/date';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import {
		api,
		errMessage,
		type Payroll,
		type Subscription,
		type Escrow,
		type Channel,
		type KeeperRun
	} from '$lib/api';
	import { toBaseUnits, assetByCode, ASSETS } from '$lib/assets';
	import { truncate } from '$lib/format';
	import { toast } from 'svelte-sonner';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';
	import DownloadIcon from '@lucide/svelte/icons/download';

	type Tab = 'payroll' | 'subscriptions' | 'channels' | 'escrow';
	// Honors a `?tab=` param so dashboard links (e.g. /payroll?tab=escrow) land on that tab.
	const tabParam = get(page).url.searchParams.get('tab') ?? '';
	let tab = $state<Tab>(
		(['payroll', 'subscriptions', 'channels', 'escrow'].includes(tabParam) ? tabParam : 'payroll') as Tab
	);
	let proving = $state(false);
	let provingTitle = $state('Working');

	// Run a proving action with the overlay, guaranteeing it always closes — even if the awaited
	// call rejects or hangs past resolution — so the UI can never get stuck behind the overlay.
	async function withProving(title: string, fn: () => Promise<unknown>) {
		proving = true;
		provingTitle = title;
		try {
			await fn();
		} finally {
			proving = false;
		}
	}

	// Inline create/edit composer — replaces the hub body with a full-width form.
	type Composer =
		| 'payroll'
		| 'subscription'
		| 'escrow-open'
		| 'escrow-contribute'
		| 'channel-open'
		| 'channel-import';
	let composer = $state<Composer | null>(null);
	const composerTitle: Record<Composer, string> = {
		payroll: 'payroll',
		subscription: 'subscription',
		'escrow-open': 'escrow',
		'escrow-contribute': 'escrow contribution',
		'channel-open': 'channel',
		'channel-import': 'channel import'
	};
	function saveComposer() {
		if (composer === 'payroll') return pSave();
		if (composer === 'subscription') return sSave();
		if (composer === 'escrow-open') return escrowOpen();
		if (composer === 'escrow-contribute') return escrowContribute();
		if (composer === 'channel-open') return channelOpen();
		if (composer === 'channel-import') return channelImport();
	}

	const tokenItems = ASSETS.map((a) => a.code);
	const decimalsOf = (code: string) =>
		wallet.balances.find((b) => b.code === code)?.decimals ?? assetByCode(code)?.decimals ?? 7;
	const scale = (base: number, code: string) =>
		(base / 10 ** decimalsOf(code)).toLocaleString('en-US', { maximumFractionDigits: 4 });
	const fmtDate = (unix: number) =>
		new Date(unix * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
	const cadenceLabel = (c: string, n: number) =>
		c === 'weekly' ? 'Weekly' : c === 'monthly' ? 'Monthly' : `Every ${n} days`;
	// Calendar (shadcn/@internationalized) <-> unix-seconds helpers.
	const tz = getLocalTimeZone();
	const todayCD = () => cdToday(tz);
	const unixToCD = (unix: number): DateValue => {
		const d = new Date(unix * 1000);
		return new CalendarDate(d.getFullYear(), d.getMonth() + 1, d.getDate());
	};
	const cdToUnix = (cd: DateValue | undefined) => (cd ? Math.floor(cd.toDate(tz).getTime() / 1000) : 0);
	const fmtCD = (cd: DateValue | undefined) =>
		cd ? cd.toDate(tz).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' }) : '';

	// This wallet's shielded payee code (for escrow share codes); loaded in onMount.
	let myCode = $state('');
	const padId = (id: number) => String(id).padStart(6, '0');
	const escrowShareCode = (e: Escrow) => `ozky-escrow:${padId(e.id)}:${myCode}:${e.asset}`;
	// A channel is sealed to the merchant's code at open time, so the subscriber only shares the id.
	const channelShareCode = (c: Channel) => `ozky-channel:${padId(c.id)}`;

	onMount(async () => {
		const tasks = [
			wallet.refreshPayrolls(),
			wallet.refreshSubscriptions(),
			wallet.refreshEscrows(),
			wallet.refreshChannels()
		];
		// Balances feed the over-balance pre-flight guard. The scan is expensive (per-commit
		// ECDH over the whole pool), so only run it here if the store is empty — if the
		// dashboard already populated balances, a slightly-stale value is fine (the backend
		// rejects an over-balance run cleanly anyway).
		if (wallet.balances.length === 0) tasks.push(wallet.refreshBalances());
		await Promise.all(tasks);
		await refreshKeeper();
		// This wallet's shielded payee code — embedded in the escrow share code so a
		// contributor scanning the QR gets the id + payee + asset in one shot.
		try {
			myCode = await api.receiveAddress();
		} catch {
			myCode = '';
		}
	});

	// ---- keeper ------------------------------------------------------------
	let keeperRuns = $state<KeeperRun[]>([]);
	async function refreshKeeper() {
		try {
			keeperRuns = await api.keeperStatus();
		} catch {
			keeperRuns = [];
		}
	}
	const armedFor = (id: number) => keeperRuns.find((k) => k.payroll_id === id);

	// ---- selection ---------------------------------------------------------
	let selId = $state<number | null>(null);
	$effect(() => {
		// Reset selection to the first row when the tab or its list changes.
		const list = currentList();
		if (list.length && !list.some((x) => x.id === selId)) selId = list[0].id;
		else if (!list.length) selId = null;
	});
	function currentList(): { id: number }[] {
		return tab === 'payroll'
			? wallet.payrolls
			: tab === 'subscriptions'
				? wallet.subscriptions
				: tab === 'channels'
					? wallet.channels
					: wallet.escrows;
	}
	const selPayroll = $derived(wallet.payrolls.find((p) => p.id === selId));
	const selSub = $derived(wallet.subscriptions.find((s) => s.id === selId));
	const selChannel = $derived(wallet.channels.find((c) => c.id === selId));
	const selEscrow = $derived(wallet.escrows.find((e) => e.id === selId));

	// ---- calendar ----------------------------------------------------------
	let calMonth = $state(new Date().getMonth());
	let calYear = $state(new Date().getFullYear());
	const events = $derived.by<CalEvent[]>(() => {
		const out: CalEvent[] = [];
		if (tab === 'payroll')
			for (const p of wallet.payrolls)
				if (p.next_run_unix)
					out.push({ ts: p.next_run_unix * 1000, label: p.label, tone: p.due ? 'due' : 'gold', selected: p.id === selId });
		if (tab === 'subscriptions')
			for (const s of wallet.subscriptions)
				if (s.next_run_unix)
					out.push({ ts: s.next_run_unix * 1000, label: s.label, tone: s.due ? 'due' : 'gold', selected: s.id === selId });
		if (tab === 'channels')
			for (const c of wallet.channels)
				if (c.expiry_unix)
					out.push({ ts: c.expiry_unix * 1000, label: `Channel #${c.id}`, tone: c.expiry_passed ? 'bad' : 'gold', selected: c.id === selId });
		if (tab === 'escrow')
			for (const e of wallet.escrows)
				if (e.deadline_unix)
					out.push({ ts: e.deadline_unix * 1000, label: `Escrow #${e.id}`, tone: e.deadline_passed ? 'bad' : 'gold', selected: e.id === selId });
		return out;
	});

	// "Due this month" total + stat triple per tab.
	const inMonth = (unix: number) => {
		const d = new Date(unix * 1000);
		return d.getMonth() === calMonth && d.getFullYear() === calYear;
	};
	const totals = $derived.by(() => {
		if (tab === 'payroll') {
			const due = wallet.payrolls.filter((p) => p.due).length;
			const month = wallet.payrolls.filter((p) => inMonth(p.next_run_unix)).length;
			const sum = wallet.payrolls.filter((p) => inMonth(p.next_run_unix)).reduce((s, p) => s + p.total, 0);
			const asset = wallet.payrolls[0]?.primary_asset ?? 'USDC';
			return { hero: scale(sum, asset), asset, a: ['Due now', due], b: ['This month', month], c: ['Armed', keeperRuns.length] as [string, number] };
		}
		if (tab === 'subscriptions') {
			const due = wallet.subscriptions.filter((s) => s.due).length;
			const month = wallet.subscriptions.filter((s) => inMonth(s.next_run_unix)).length;
			const sum = wallet.subscriptions.filter((s) => inMonth(s.next_run_unix)).reduce((s, x) => s + x.amount, 0);
			const asset = wallet.subscriptions[0]?.asset ?? 'USDC';
			return { hero: scale(sum, asset), asset, a: ['Due now', due], b: ['This month', month], c: ['Active', wallet.subscriptions.filter((s) => s.enabled).length] as [string, number] };
		}
		if (tab === 'channels') {
			const open = wallet.channels.filter((c) => c.status === 'open').length;
			return { hero: String(wallet.channels.length), asset: 'channels', a: ['Open', open], b: ['Closeable', wallet.channels.filter((c) => c.closeable).length], c: ['Reclaimable', wallet.channels.filter((c) => c.reclaimable).length] as [string, number] };
		}
		const open = wallet.escrows.filter((e) => e.status === 'open').length;
		return { hero: String(wallet.escrows.length), asset: 'escrows', a: ['Open', open], b: ['Releasable', wallet.escrows.filter((e) => e.releasable).length], c: ['Refundable', wallet.escrows.filter((e) => e.refundable).length] as [string, number] };
	});

	// ---- lifecycle actions -------------------------------------------------
	let confirm = $state<{ title: string; body: string; run: () => Promise<void> } | null>(null);
	function ask(title: string, body: string, run: () => Promise<void>) {
		confirm = { title, body, run };
	}

	// Available shielded balance (base units) for an asset.
	const availRaw = (code: string) => wallet.balances.find((b) => b.code === code)?.raw ?? 0;
	function enoughBalance(asset: string, needBase: number): boolean {
		if (needBase > availRaw(asset)) {
			const bal = wallet.balances.find((b) => b.code === asset);
			toast.error(`Insufficient shielded ${asset}`, {
				description: `Needs ${scale(needBase, asset)} ${asset}, available ${bal?.display ?? '0'}.`
			});
			return false;
		}
		return true;
	}

	async function runPayroll(p: Payroll) {
		// Guard: a run whose total exceeds the shielded balance can't be proven and would
		// otherwise hang the proving overlay. Each funding group draws from its own asset,
		// so validate every group before entering the overlay.
		for (const g of p.groups) if (!enoughBalance(g.asset, g.total)) return;
		await withProving(`Running payroll "${p.label}"`, () => wallet.runPayroll(p.id));
	}
	// A split run needs ONE note that covers a group's whole total. If funds are fragmented
	// (the "no single owned note covers split total" error), merge each funding asset's notes
	// into one, then the user can run again.
	async function consolidateFunds(p: Payroll) {
		const assets = [...new Set(p.groups.map((g) => g.asset))];
		try {
			for (const a of assets) {
				await withProving(`Consolidating ${a} notes`, async () => {
					try {
						await api.consolidate(a);
					} catch (e) {
						// "need >= 2 notes" just means nothing to merge for this asset — skip it.
						if (!/>=\s*2 notes|nothing to consolidate/i.test(errMessage(e))) throw e;
					}
				});
			}
			await wallet.refreshBalances();
			toast.success('Notes consolidated — try the run again');
		} catch (e) {
			toast.error('Could not consolidate', { description: errMessage(e) });
		}
	}
	async function runSub(s: Subscription) {
		if (!enoughBalance(s.asset, s.amount)) return;
		await withProving(`Charging "${s.label}"`, () => wallet.runSubscription(s.id));
	}
	async function release(e: Escrow) {
		await withProving(`Releasing escrow #${e.id}`, () => wallet.releaseEscrow(e.id));
		await wallet.refreshEscrows();
	}
	async function refund(e: Escrow, idx: number) {
		await withProving(`Refunding escrow #${e.id}`, () => wallet.refundEscrow(e.id, idx));
		await wallet.refreshEscrows();
	}
	async function closeCh(c: Channel) {
		await withProving(`Closing channel #${c.id}`, () => wallet.closeChannel(c.id));
		await wallet.refreshChannels();
	}
	async function reclaimCh(c: Channel) {
		await withProving(`Reclaiming channel #${c.id}`, () => wallet.reclaimChannel(c.id));
		await wallet.refreshChannels();
	}
	async function toggleEnabled(kind: 'payroll' | 'sub', id: number, enabled: boolean) {
		try {
			if (kind === 'payroll') await api.setPayrollEnabled(id, enabled);
			else await api.setSubscriptionEnabled(id, enabled);
			if (kind === 'payroll') await wallet.refreshPayrolls();
			else await wallet.refreshSubscriptions();
		} catch (e) {
			toast.error('Could not update', { description: errMessage(e) });
		}
	}
	async function del(kind: 'payroll' | 'sub', id: number) {
		try {
			if (kind === 'payroll') await api.deletePayroll(id);
			else await api.deleteSubscription(id);
			if (kind === 'payroll') await wallet.refreshPayrolls();
			else await wallet.refreshSubscriptions();
		} catch (e) {
			toast.error('Could not delete', { description: errMessage(e) });
		}
	}
	async function armKeeper(p: Payroll) {
		try {
			await api.armPayrollKeeper(p.id);
			await refreshKeeper();
			toast.success('Headless run armed');
		} catch (e) {
			toast.error('Could not arm', { description: errMessage(e) });
		}
	}
	async function disarmKeeper(p: Payroll) {
		try {
			await api.disarmPayrollKeeper(p.id);
			await refreshKeeper();
			toast.success('Disarmed');
		} catch (e) {
			toast.error('Could not disarm', { description: errMessage(e) });
		}
	}

	// ---- create / edit dialogs --------------------------------------------
	// Payroll — multi-token: one tab per funding asset, each holding its own payee list.
	// Rows are kept per-asset; on save, every asset with ≥1 valid payee becomes a group.
	type PRow = { code: string; amount: string };
	function emptyRows(): Record<string, PRow[]> {
		const m: Record<string, PRow[]> = {};
		for (const a of tokenItems) m[a] = [{ code: '', amount: '' }];
		return m;
	}
	let pEditId = $state(0);
	let pLabel = $state('');
	let pCadence = $state('weekly');
	let pInterval = $state('14');
	let pStart = $state<DateValue | undefined>(todayCD());
	let pEnd = $state<DateValue | undefined>(undefined);
	let pAuditor = $state('');
	let pApproval = $state('manual');
	let pRunLocation = $state('local');
	let pRowsByAsset = $state<Record<string, PRow[]>>(emptyRows());
	let pActiveAsset = $state(tokenItems[0]);
	const pPayeeCount = (a: string) =>
		(pRowsByAsset[a] ?? []).filter((r) => r.code.trim().startsWith('ozky') && Number(r.amount) > 0).length;
	function pCreate() {
		pEditId = 0;
		pLabel = '';
		pCadence = 'weekly';
		pInterval = '14';
		pStart = todayCD();
		pEnd = undefined;
		pAuditor = '';
		pApproval = 'manual';
		pRunLocation = 'local';
		pRowsByAsset = emptyRows();
		pActiveAsset = tokenItems[0];
		composer = 'payroll';
	}
	function pEdit(p: Payroll) {
		pEditId = p.id;
		pLabel = p.label;
		pCadence = p.cadence;
		pInterval = String(p.interval_days || 14);
		pStart = p.next_run_unix ? unixToCD(p.next_run_unix) : todayCD();
		pEnd = p.end_unix ? unixToCD(p.end_unix) : undefined;
		pAuditor = p.auditor ?? '';
		pApproval = p.approval ?? 'manual';
		pRunLocation = p.run_location ?? 'local';
		const m = emptyRows();
		for (const g of p.groups) {
			m[g.asset] = g.payees.length
				? g.payees.map((x) => ({ code: x.code, amount: scale(x.amount, x.recv_asset ?? g.asset) }))
				: [{ code: '', amount: '' }];
		}
		pRowsByAsset = m;
		pActiveAsset = p.groups[0]?.asset ?? tokenItems[0];
		composer = 'payroll';
	}
	async function pSave() {
		if (!pLabel.trim()) return toast.error('Give the payroll a name');
		const groups: { asset: string; payees: { code: string; amount: number }[] }[] = [];
		try {
			for (const asset of tokenItems) {
				const valid = (pRowsByAsset[asset] ?? []).filter(
					(r) => r.code.trim().startsWith('ozky') && Number(r.amount) > 0
				);
				if (!valid.length) continue;
				groups.push({
					asset,
					payees: valid.map((r) => ({ code: r.code.trim(), amount: toBaseUnits(r.amount, decimalsOf(asset)) }))
				});
			}
		} catch (e) {
			return toast.error(errMessage(e));
		}
		if (!groups.length) return toast.error('Add at least one payee with an ozky… code');
		try {
			await api.savePayroll({
				id: pEditId,
				label: pLabel.trim(),
				groups,
				cadence: pCadence,
				interval_days: pCadence === 'days' ? Math.max(1, Number(pInterval) || 1) : 0,
				start_unix: cdToUnix(pStart) || Math.floor(Date.now() / 1000),
				end_unix: cdToUnix(pEnd),
				auditor: pAuditor.trim(),
				approval: pApproval,
				run_location: pRunLocation
			});
			composer = null;
			await wallet.refreshPayrolls();
			toast.success('Payroll saved');
		} catch (e) {
			toast.error('Could not save', { description: errMessage(e) });
		}
	}

	// Subscription
	let sEditId = $state(0);
	let sLabel = $state('');
	let sAsset = $state('USDC');
	let sCode = $state('');
	let sAmount = $state('');
	let sCadence = $state('monthly');
	let sInterval = $state('30');
	let sStart = $state<DateValue | undefined>(todayCD());
	let sEnd = $state<DateValue | undefined>(undefined);
	let sAuditor = $state('');
	let sApproval = $state('manual');
	let sRunLocation = $state('local');
	function sCreate() {
		sEditId = 0;
		sLabel = '';
		sAsset = 'USDC';
		sCode = '';
		sAmount = '';
		sCadence = 'monthly';
		sInterval = '30';
		sStart = todayCD();
		sEnd = undefined;
		sAuditor = '';
		sApproval = 'manual';
		sRunLocation = 'local';
		composer = 'subscription';
	}
	function sEdit(s: Subscription) {
		sEditId = s.id;
		sLabel = s.label;
		sAsset = s.asset;
		sCode = s.code;
		sAmount = scale(s.amount, s.asset);
		sCadence = s.cadence;
		sInterval = String(s.interval_days || 30);
		sStart = s.next_run_unix ? unixToCD(s.next_run_unix) : todayCD();
		sEnd = s.end_unix ? unixToCD(s.end_unix) : undefined;
		sAuditor = s.auditor ?? '';
		sApproval = s.approval ?? 'manual';
		sRunLocation = s.run_location ?? 'local';
		composer = 'subscription';
	}
	async function sSave() {
		if (!sLabel.trim()) return toast.error('Give the subscription a name');
		if (!sCode.trim().startsWith('ozky')) return toast.error('Enter a valid ozky… recipient code');
		if (!(Number(sAmount) > 0)) return toast.error('Enter an amount greater than zero');
		let base: number;
		try {
			base = toBaseUnits(sAmount, decimalsOf(sAsset));
		} catch (e) {
			return toast.error(errMessage(e));
		}
		try {
			await api.saveSubscription({
				id: sEditId,
				label: sLabel.trim(),
				asset: sAsset,
				code: sCode.trim(),
				amount: base,
				cadence: sCadence,
				interval_days: sCadence === 'days' ? Math.max(1, Number(sInterval) || 1) : 0,
				start_unix: cdToUnix(sStart) || Math.floor(Date.now() / 1000),
				end_unix: cdToUnix(sEnd),
				auditor: sAuditor.trim(),
				approval: sApproval,
				run_location: sRunLocation
			});
			composer = null;
			await wallet.refreshSubscriptions();
			toast.success('Subscription saved');
		} catch (e) {
			toast.error('Could not save', { description: errMessage(e) });
		}
	}

	// Delete-from-composer (only when editing an existing payroll/subscription).
	const composerEditId = $derived(
		composer === 'payroll' ? pEditId : composer === 'subscription' ? sEditId : 0
	);
	function deleteComposer() {
		const kind = composer === 'payroll' ? 'payroll' : 'sub';
		const id = composerEditId;
		const name = composer === 'payroll' ? pLabel : sLabel;
		ask(`Delete ${kind === 'payroll' ? 'payroll' : 'subscription'}`, `Delete "${name}"? This can't be undone.`, async () => {
			await del(kind, id);
			composer = null;
		});
	}

	// Escrow open + contribute
	let eoAsset = $state('USDC');
	let eoTarget = $state('');
	let eoDeadline = $state('');
	let eoMode = $state('all_or_nothing');
	async function escrowOpen() {
		if (!(Number(eoTarget) > 0)) return toast.error('Enter a target greater than zero');
		if (!eoDeadline) return toast.error('Pick a deadline');
		let base: number;
		try {
			base = toBaseUnits(eoTarget, decimalsOf(eoAsset));
		} catch (e) {
			return toast.error(errMessage(e));
		}
		const deadlineUnix = Math.floor(new Date(eoDeadline).getTime() / 1000);
		if (deadlineUnix <= Date.now() / 1000) return toast.error('Deadline must be in the future');
		try {
			await wallet.openEscrow(eoAsset, base, deadlineUnix, eoMode);
			composer = null;
			await wallet.refreshEscrows();
			// Surface the just-created escrow (its id + QR) in the detail panel.
			if (wallet.escrows.length) selId = Math.max(...wallet.escrows.map((x) => x.id));
		} catch (e) {
			toast.error('Could not open', { description: errMessage(e) });
		}
	}
	let ecPaste = $state('');
	let ecId = $state('');
	let ecCode = $state('');
	let ecAsset = $state('');
	let ecAmount = $state('');
	// Parse a shared escrow code `ozky-escrow:<id6>:<payeeCode>:<asset>` into the contribute fields.
	function applyEscrowCode() {
		const m = ecPaste.trim().match(/^ozky-escrow:(\d+):(ozky[^:]+):([^:]+)$/i);
		if (!m) return;
		ecId = String(Number(m[1]));
		ecCode = m[2];
		ecAsset = m[3];
	}
	async function escrowContribute() {
		const id = Number(ecId);
		if (!(id > 0)) return toast.error('Enter the escrow id');
		if (!ecCode.trim().startsWith('ozky')) return toast.error('Enter the payee’s ozky… code');
		if (!(Number(ecAmount) > 0)) return toast.error('Enter an amount greater than zero');
		let base: number;
		try {
			base = toBaseUnits(ecAmount, decimalsOf(ecAsset || 'USDC'));
		} catch (e) {
			return toast.error(errMessage(e));
		}
		await withProving(`Contributing to escrow #${id}`, () =>
			wallet.contributeEscrow(id, ecCode.trim(), base)
		);
		composer = null;
		await wallet.refreshEscrows();
	}

	// Channel open + import
	let coAsset = $state('USDC');
	let coMerchant = $state('');
	let coPer = $state('');
	let coPeriods = $state('12');
	let coIntervalDays = $state('30');
	async function channelOpen() {
		if (!coMerchant.trim().startsWith('ozky')) return toast.error('Enter the merchant’s ozky… code');
		if (!(Number(coPer) > 0)) return toast.error('Enter a per-period amount');
		const periods = Math.max(1, Number(coPeriods) || 1);
		let perBase: number, capBase: number;
		try {
			perBase = toBaseUnits(coPer, decimalsOf(coAsset));
			capBase = perBase * periods;
		} catch (e) {
			return toast.error(errMessage(e));
		}
		await withProving('Opening channel', () =>
			wallet.openChannel(coAsset, capBase, coMerchant.trim(), perBase, periods, Math.max(1, Number(coIntervalDays) || 1) * 86400)
		);
		composer = null;
		await wallet.refreshChannels();
	}
	let ciId = $state('');
	let ciPaste = $state('');
	// Accept a shared `ozky-channel:<id6>` code or a bare number, filling the channel id.
	function applyChannelCode() {
		const m = ciPaste.trim().match(/(?:ozky-channel:)?(\d+)/i);
		if (m) ciId = String(Number(m[1]));
	}
	async function channelImport() {
		const id = Number(ciId);
		if (!(id > 0)) return toast.error('Enter the channel id');
		try {
			await wallet.importChannel(id);
			composer = null;
			await wallet.refreshChannels();
			toast.success('Channel imported');
		} catch (e) {
			toast.error('Could not import', { description: errMessage(e) });
		}
	}

	function addPRow() {
		const r = pRowsByAsset[pActiveAsset];
		if (r && r.length < 25) r.push({ code: '', amount: '' });
	}
	function rmPRow(i: number) {
		pRowsByAsset[pActiveAsset]?.splice(i, 1);
	}
	const cadenceText = (c: string) => (c === 'weekly' ? 'Weekly' : c === 'monthly' ? 'Monthly' : 'Every N days');

	const tabs: { value: Tab; label: string }[] = [
		{ value: 'payroll', label: 'Payroll' },
		{ value: 'subscriptions', label: 'Subscriptions' },
		{ value: 'channels', label: 'Channels' },
		{ value: 'escrow', label: 'Escrow' }
	];
</script>

<div class="hub">
	<!-- segmented tabs (navbar-pill style) + per-tab primary action -->
	<div class="topbar">
		<Tabs.Root bind:value={tab}>
			<Tabs.List class="grid w-fit grid-cols-4 gap-1 rounded-full border border-border bg-card/50 p-1">
				{#each tabs as t (t.value)}
					<Tabs.Trigger value={t.value} class="pilltab">{t.label}</Tabs.Trigger>
				{/each}
			</Tabs.List>
		</Tabs.Root>
		<div class="actions">
			{#if composer}
				{#if composerEditId}
					<Button variant="ghost" size="sm" class="text-destructive" onclick={deleteComposer}>Delete</Button>
				{/if}
				<Button variant="outline" size="sm" onclick={() => (composer = null)}>Cancel</Button>
				<Button size="sm" onclick={saveComposer}>{composer === 'channel-import' ? 'Import' : composer === 'escrow-contribute' ? 'Contribute' : 'Save'}</Button>
			{:else if tab === 'payroll'}
				<Button size="sm" onclick={pCreate}><PlusIcon data-icon="inline-start" />New payroll</Button>
			{:else if tab === 'subscriptions'}
				<Button size="sm" onclick={sCreate}><PlusIcon data-icon="inline-start" />New subscription</Button>
			{:else if tab === 'channels'}
				<Button variant="outline" size="sm" onclick={() => (composer = 'channel-import')}><DownloadIcon data-icon="inline-start" />Import</Button>
				<Button size="sm" onclick={() => (composer = 'channel-open')}><PlusIcon data-icon="inline-start" />New channel</Button>
			{:else}
				<Button variant="outline" size="sm" onclick={() => (composer = 'escrow-contribute')}>Contribute</Button>
				<Button size="sm" onclick={() => (composer = 'escrow-open')}><PlusIcon data-icon="inline-start" />New escrow</Button>
			{/if}
		</div>
	</div>

	{#if composer === 'payroll' || composer === 'subscription'}
		<section class="card composer" in:fly={{ y: 12, duration: 280, easing: cubicOut }}>
			<h2 class="comp-title">{(composer === 'payroll' && pEditId) || (composer === 'subscription' && sEditId) ? 'Edit' : 'New'} {composerTitle[composer]}</h2>
			{#snippet dateField(label: string, value: DateValue | undefined, set: (v: DateValue | undefined) => void, optional: boolean, min: DateValue | undefined)}
				<Field.Field>
					<Field.Label>{label}{#if optional}<span class="opt">optional</span>{/if}</Field.Label>
					<div class="date-row">
						<Popover.Root>
							<Popover.Trigger class="date-btn">
								<CalendarIcon class="size-4 opacity-70" />
								<span data-empty={!value}>{value ? fmtCD(value) : optional ? 'No end date' : 'Pick a date'}</span>
							</Popover.Trigger>
							<Popover.Content class="w-auto overflow-hidden p-0" align="start">
								<Calendar type="single" {value} onValueChange={set} captionLayout="dropdown" minValue={min} />
							</Popover.Content>
						</Popover.Root>
						{#if optional && value}<button type="button" class="linkbtn" onclick={() => set(undefined)}>Clear</button>{/if}
					</div>
				</Field.Field>
			{/snippet}
			{#if composer === 'payroll'}
				<div class="compose2">
					<div class="col-left">
						<Field.Field><Field.Label>Name</Field.Label><Input bind:value={pLabel} placeholder="e.g. Engineering" /></Field.Field>
						<Field.Field>
							<Field.Label>Cadence</Field.Label>
							<Select.Root type="single" bind:value={pCadence}>
								<Select.Trigger class="bg-popover">{cadenceText(pCadence)}</Select.Trigger>
								<Select.Content class="bg-popover"><Select.Item value="weekly">Weekly · every 7 days</Select.Item><Select.Item value="monthly">Monthly · on the start date</Select.Item><Select.Item value="days">Every N days</Select.Item></Select.Content>
							</Select.Root>
						</Field.Field>
						<div class="two">
							{@render dateField('Start date', pStart, (v) => (pStart = v), false, todayCD())}
							{@render dateField('End date', pEnd, (v) => (pEnd = v), true, pStart ?? todayCD())}
						</div>
						{#if pCadence === 'days'}<Field.Field><Field.Label>Repeat every (days)</Field.Label><Input bind:value={pInterval} inputmode="numeric" /></Field.Field>{/if}
						<div class="field-sep"></div>
						<div class="two">
							<Field.Field>
								<Field.Label>Approval</Field.Label>
								<ToggleGroup.Root type="single" bind:value={pApproval} class="grid grid-cols-2">
									<ToggleGroup.Item value="manual" class="text-xs">Manual</ToggleGroup.Item>
									<ToggleGroup.Item value="auto" class="text-xs">Auto-run</ToggleGroup.Item>
								</ToggleGroup.Root>
							</Field.Field>
							<Field.Field>
								<Field.Label>Run on</Field.Label>
								<ToggleGroup.Root type="single" bind:value={pRunLocation} class="grid grid-cols-2">
									<ToggleGroup.Item value="local" class="text-xs">This device</ToggleGroup.Item>
									<ToggleGroup.Item value="cloud" class="text-xs">Cloud</ToggleGroup.Item>
								</ToggleGroup.Root>
							</Field.Field>
						</div>
						<Field.Field>
							<Field.Label>Auditor <span class="opt">optional</span></Field.Label>
							<Input bind:value={pAuditor} placeholder="G… auditor address" class="font-mono" />
						</Field.Field>
					</div>
					<div class="col-right">
						<div class="token-tabs">
							{#each tokenItems as t (t)}
								<button type="button" class="ttab" data-active={t === pActiveAsset} onclick={() => (pActiveAsset = t)}>
									{t}{#if pPayeeCount(t)}<span class="ttab-n">{pPayeeCount(t)}</span>{/if}
								</button>
							{/each}
						</div>
						<div class="payee-scroll">
							<div class="payee-table">
								{#each pRowsByAsset[pActiveAsset] as row, i (i)}
									<div class="payee-edit">
										<Input bind:value={row.code} placeholder="ozky…" class="flex-1 font-mono" />
										<Input bind:value={row.amount} placeholder="0.00" inputmode="decimal" class="w-28 font-mono" />
										<span class="w-12 text-sm text-muted-foreground">{pActiveAsset}</span>
										{#if pRowsByAsset[pActiveAsset].length > 1}<button class="del" onclick={() => rmPRow(i)} aria-label="Remove"><XIcon class="size-4" /></button>{/if}
									</div>
								{/each}
							</div>
							<Button variant="ghost" size="sm" class="w-fit" onclick={addPRow}><PlusIcon data-icon="inline-start" />Add payee</Button>
						</div>
					</div>
				</div>
			{:else}
				<div class="compose2">
					<div class="col-left">
						<Field.Field><Field.Label>Name</Field.Label><Input bind:value={sLabel} placeholder="e.g. Cloud hosting" /></Field.Field>
						<Field.Field>
							<Field.Label>Cadence</Field.Label>
							<Select.Root type="single" bind:value={sCadence}>
								<Select.Trigger class="bg-popover">{cadenceText(sCadence)}</Select.Trigger>
								<Select.Content class="bg-popover"><Select.Item value="weekly">Weekly · every 7 days</Select.Item><Select.Item value="monthly">Monthly · on the start date</Select.Item><Select.Item value="days">Every N days</Select.Item></Select.Content>
							</Select.Root>
						</Field.Field>
						{#if sCadence === 'days'}<Field.Field><Field.Label>Repeat every (days)</Field.Label><Input bind:value={sInterval} inputmode="numeric" /></Field.Field>{/if}
						<Field.Field><Field.Label>Merchant code</Field.Label><Input bind:value={sCode} placeholder="ozky…" class="font-mono" /></Field.Field>
						<div class="two">
							<Field.Field>
								<Field.Label>Asset</Field.Label>
								<Select.Root type="single" bind:value={sAsset}>
									<Select.Trigger class="bg-popover">{sAsset}</Select.Trigger>
									<Select.Content class="bg-popover">{#each tokenItems as t (t)}<Select.Item value={t}>{t}</Select.Item>{/each}</Select.Content>
								</Select.Root>
							</Field.Field>
							<Field.Field><Field.Label>Amount</Field.Label><Input bind:value={sAmount} placeholder="0.00" inputmode="decimal" class="font-mono" /></Field.Field>
						</div>
					</div>
					<div class="col-right">
						<div class="two">
							{@render dateField('First charge', sStart, (v) => (sStart = v), false, todayCD())}
							{@render dateField('End date', sEnd, (v) => (sEnd = v), true, sStart ?? todayCD())}
						</div>
						<div class="two">
							<Field.Field>
								<Field.Label>Approval</Field.Label>
								<ToggleGroup.Root type="single" bind:value={sApproval} class="grid grid-cols-2">
									<ToggleGroup.Item value="manual" class="text-xs">Manual</ToggleGroup.Item>
									<ToggleGroup.Item value="auto" class="text-xs">Auto-run</ToggleGroup.Item>
								</ToggleGroup.Root>
							</Field.Field>
							<Field.Field>
								<Field.Label>Run on</Field.Label>
								<ToggleGroup.Root type="single" bind:value={sRunLocation} class="grid grid-cols-2">
									<ToggleGroup.Item value="local" class="text-xs">This device</ToggleGroup.Item>
									<ToggleGroup.Item value="cloud" class="text-xs">Cloud</ToggleGroup.Item>
								</ToggleGroup.Root>
							</Field.Field>
						</div>
						<Field.Field>
							<Field.Label>Auditor <span class="opt">optional</span></Field.Label>
							<Input bind:value={sAuditor} placeholder="G… auditor address" class="font-mono" />
						</Field.Field>
					</div>
				</div>
			{/if}
		</section>
	{:else}

	<div class="cols" in:fly={{ y: 12, duration: 320, easing: cubicOut }}>
		<!-- LEFT: schedule list -->
		<aside class="card list">
			{#if currentList().length === 0}
				<Empty.Root class="rounded-2xl border border-dashed py-12">
					<Empty.Content><Empty.Description>No {tab} yet.</Empty.Description></Empty.Content>
				</Empty.Root>
			{:else}
				{#if tab === 'payroll'}
					{#each wallet.payrolls as p (p.id)}
						<button class="row" data-active={p.id === selId} onclick={() => (selId = p.id)}>
							<AccountAvatar seed={p.label} size={32} />
							<div class="min-w-0 flex-1 text-left">
								<div class="truncate text-sm font-medium">{p.label}</div>
								<div class="truncate text-xs text-muted-foreground">{cadenceLabel(p.cadence, p.interval_days)} · {p.payee_count} payees</div>
							</div>
							{#if p.due}<Badge>Due</Badge>{:else if !p.enabled}<Badge variant="secondary">Paused</Badge>{/if}
						</button>
					{/each}
				{:else if tab === 'subscriptions'}
					{#each wallet.subscriptions as s (s.id)}
						<button class="row" data-active={s.id === selId} onclick={() => (selId = s.id)}>
							<AccountAvatar seed={s.label} size={32} />
							<div class="min-w-0 flex-1 text-left">
								<div class="truncate text-sm font-medium">{s.label}</div>
								<div class="truncate text-xs text-muted-foreground">{scale(s.amount, s.asset)} {s.asset} · {cadenceLabel(s.cadence, s.interval_days)}</div>
							</div>
							{#if s.due}<Badge>Due</Badge>{:else if !s.enabled}<Badge variant="secondary">Paused</Badge>{/if}
						</button>
					{/each}
				{:else if tab === 'channels'}
					{#each wallet.channels as c (c.id)}
						<button class="row" data-active={c.id === selId} onclick={() => (selId = c.id)}>
							<AccountAvatar seed={`channel${c.id}`} size={32} />
							<div class="min-w-0 flex-1 text-left">
								<div class="truncate text-sm font-medium">Channel #{padId(c.id)}</div>
								<div class="truncate text-xs text-muted-foreground">{c.is_merchant ? 'You charge' : 'You pay'} · {c.asset}</div>
							</div>
							{#if c.expiry_passed}<Badge variant="destructive">Expired</Badge>{:else if c.status === 'closed'}<Badge variant="secondary">Closed</Badge>{/if}
						</button>
					{/each}
				{:else}
					{#each wallet.escrows as e (e.id)}
						<button class="row" data-active={e.id === selId} onclick={() => (selId = e.id)}>
							<AccountAvatar seed={`escrow${e.id}`} size={32} />
							<div class="min-w-0 flex-1 text-left">
								<div class="truncate text-sm font-medium">Escrow #{padId(e.id)}</div>
								<div class="truncate text-xs text-muted-foreground">Target {scale(e.target, e.asset)} {e.asset}</div>
							</div>
							{#if e.releasable}<Badge>Releasable</Badge>{:else if e.refundable}<Badge variant="destructive">Refundable</Badge>{:else if e.deadline_passed}<Badge variant="destructive">Ended</Badge>{/if}
						</button>
					{/each}
				{/if}
			{/if}
		</aside>

		<!-- CENTER: totals + calendar -->
		<section class="center">
			<div class="emphasis">
				<div class="emph-row">
					<div>
						<span class="muted-label">{tab === 'payroll' || tab === 'subscriptions' ? 'Due this month' : 'Total'}</span>
						<div class="hero">{totals.hero} <span class="hero-unit">{totals.asset}</span></div>
					</div>
				</div>
				<div class="stats">
					<div class="stat"><span class="stat-n">{totals.a[1]}</span><span class="stat-l">{totals.a[0]}</span></div>
					<div class="stat"><span class="stat-n">{totals.b[1]}</span><span class="stat-l">{totals.b[0]}</span></div>
					<div class="stat"><span class="stat-n">{totals.c[1]}</span><span class="stat-l">{totals.c[0]}</span></div>
				</div>
			</div>
			<div class="card cal-card">
				<CalendarMonth {events} bind:month={calMonth} bind:year={calYear} />
			</div>
		</section>

		<!-- RIGHT: detail -->
		<aside class="card detail">
			{#if composer === 'escrow-open'}
				<h2 class="detail-form-title">New escrow</h2>
				<div class="detail-form">
					<Field.Field>
						<Field.Label>Asset</Field.Label>
						<Select.Root type="single" bind:value={eoAsset}>
							<Select.Trigger class="bg-popover">{eoAsset}</Select.Trigger>
							<Select.Content class="bg-popover">{#each tokenItems as t (t)}<Select.Item value={t}>{t}</Select.Item>{/each}</Select.Content>
						</Select.Root>
					</Field.Field>
					<Field.Field><Field.Label>Target</Field.Label><Input bind:value={eoTarget} placeholder="0.00" inputmode="decimal" class="font-mono" /></Field.Field>
					<Field.Field><Field.Label>Deadline</Field.Label><Input type="date" bind:value={eoDeadline} /></Field.Field>
					<Field.Field>
						<Field.Label>Mode</Field.Label>
						<Select.Root type="single" bind:value={eoMode}>
							<Select.Trigger class="bg-popover">{eoMode === 'all_or_nothing' ? 'All or nothing' : 'Keep what you raise'}</Select.Trigger>
							<Select.Content class="bg-popover"><Select.Item value="all_or_nothing">All or nothing</Select.Item><Select.Item value="keep_what_you_raise">Keep what you raise</Select.Item></Select.Content>
						</Select.Root>
					</Field.Field>
				</div>
			{:else if composer === 'escrow-contribute'}
				<h2 class="detail-form-title">Contribute to escrow</h2>
				<div class="detail-form">
					<Field.Field>
						<Field.Label>Paste escrow code</Field.Label>
						<Input bind:value={ecPaste} oninput={applyEscrowCode} placeholder="ozky-escrow:…" class="font-mono" />
						<p class="hint">Scan or paste the creator's escrow code to fill the fields below.</p>
					</Field.Field>
					<div class="field-sep"></div>
					<Field.Field><Field.Label>Escrow id</Field.Label><Input bind:value={ecId} inputmode="numeric" /></Field.Field>
					<Field.Field><Field.Label>Payee code</Field.Label><Input bind:value={ecCode} placeholder="ozky…" class="font-mono" /></Field.Field>
					<Field.Field><Field.Label>Amount {ecAsset ? `(${ecAsset})` : ''}</Field.Label><Input bind:value={ecAmount} placeholder="0.00" inputmode="decimal" class="font-mono" /></Field.Field>
				</div>
			{:else if composer === 'channel-open'}
				<h2 class="detail-form-title">Open a channel</h2>
				<div class="detail-form">
					<p class="hint">You're the <b>subscriber</b>: lock funds now so a merchant can pull a fixed amount each period. Unused balance is reclaimable after expiry; share the channel id with the merchant after opening.</p>
					<Field.Field><Field.Label>Merchant code</Field.Label><Input bind:value={coMerchant} placeholder="ozky…" class="font-mono" /></Field.Field>
					<Field.Field>
						<Field.Label>Asset</Field.Label>
						<Select.Root type="single" bind:value={coAsset}>
							<Select.Trigger class="bg-popover">{coAsset}</Select.Trigger>
							<Select.Content class="bg-popover">{#each tokenItems as t (t)}<Select.Item value={t}>{t}</Select.Item>{/each}</Select.Content>
						</Select.Root>
					</Field.Field>
					<Field.Field><Field.Label>Per period</Field.Label><Input bind:value={coPer} placeholder="0.00" inputmode="decimal" class="font-mono" /></Field.Field>
					<div class="two">
						<Field.Field><Field.Label>Periods</Field.Label><Input bind:value={coPeriods} inputmode="numeric" /></Field.Field>
						<Field.Field><Field.Label>Period (days)</Field.Label><Input bind:value={coIntervalDays} inputmode="numeric" /></Field.Field>
					</div>
				</div>
			{:else if composer === 'channel-import'}
				<h2 class="detail-form-title">Import a channel</h2>
				<div class="detail-form">
					<p class="hint">You're the <b>merchant</b>: import a channel a subscriber opened to you to track and collect it.</p>
					<Field.Field>
						<Field.Label>Paste channel code</Field.Label>
						<Input bind:value={ciPaste} oninput={applyChannelCode} placeholder="ozky-channel:… or id" class="font-mono" />
					</Field.Field>
					<div class="field-sep"></div>
					<Field.Field><Field.Label>Channel id</Field.Label><Input bind:value={ciId} inputmode="numeric" /></Field.Field>
				</div>
			{:else if tab === 'payroll' && selPayroll}
				{@const p = selPayroll}
				<div class="detail-head">
					<AccountAvatar seed={p.label} size={40} />
					<div class="min-w-0 flex-1">
						<h2 class="truncate font-heading text-lg font-semibold">{p.label}</h2>
						<p class="text-xs text-muted-foreground">{cadenceLabel(p.cadence, p.interval_days)} · next {fmtDate(p.next_run_unix)}</p>
					</div>
				</div>
				<div class="kv">
					<div><dt>Funds from</dt><dd>{p.groups.map((g) => g.asset).join(', ') || '—'}</dd></div>
					<div><dt>Payees</dt><dd>{p.payee_count}</dd></div>
					<div><dt>Ends</dt><dd>{p.end_unix ? fmtDate(p.end_unix) : 'No end'}</dd></div>
					<div><dt>Last run</dt><dd>{p.last_run_unix ? fmtDate(p.last_run_unix) : '—'}</dd></div>
				</div>
				<div class="kv">
					<div><dt>Approval</dt><dd>{p.approval === 'auto' ? 'Auto-run when due' : 'Manual'}</dd></div>
					<div><dt>Runs on</dt><dd>{p.run_location === 'cloud' ? 'Cloud keeper' : 'This device'}</dd></div>
				</div>
				{#if p.auditor}
					<div class="kv"><div><dt>Auditor</dt><dd class="font-mono">{truncate(p.auditor, 6, 4)}</dd></div></div>
				{/if}
				{#each p.groups as g, gi (gi)}
					<div class="sub-h">{g.asset} · {scale(g.total, g.asset)} {g.asset}</div>
					<div class="payees">
						{#each g.payees as py, i (i)}
							<div class="payee">
								<AccountAvatar seed={py.code} size={22} />
								<span class="flex-1 truncate font-mono text-xs">{truncate(py.code, 8, 6)}</span>
								<span class="font-mono text-xs">{scale(py.amount, py.recv_asset ?? g.asset)}{py.recv_asset && py.recv_asset !== g.asset ? ' ' + py.recv_asset : ''}</span>
							</div>
						{/each}
					</div>
				{/each}
				<div class="sub-h">Headless keeper</div>
				{#if armedFor(p.id)}
					{@const k = armedFor(p.id)!}
					<div class="kv">
						<div><dt>Status</dt><dd><Badge>Armed</Badge></dd></div>
						<div><dt>Epoch</dt><dd>{k.bound_epoch}</dd></div>
						<div><dt>Submitted</dt><dd>{k.submitted}/{k.chunks}</dd></div>
					</div>
					{#if k.error}<p class="text-xs text-destructive">{k.error}</p>{/if}
					<Button variant="outline" size="sm" class="mt-1 w-fit" onclick={() => disarmKeeper(p)}>Disarm</Button>
				{:else}
					<Button variant="outline" size="sm" class="w-fit" onclick={() => armKeeper(p)} disabled={!p.enabled}>Run headless</Button>
				{/if}
				<div class="act">
					<Button class="flex-1" onclick={() => ask('Run payroll', `Pay all ${p.payee_count} payees now?`, () => runPayroll(p))}>Run now</Button>
					<Button variant="outline" onclick={() => pEdit(p)}>Edit</Button>
					<Button variant="outline" onclick={() => toggleEnabled('payroll', p.id, !p.enabled)}>{p.enabled ? 'Pause' : 'Resume'}</Button>
					<Button variant="ghost" onclick={() => ask('Delete payroll', `Delete "${p.label}"?`, async () => del('payroll', p.id))}>Delete</Button>
				</div>
				<button class="consolidate-hint" onclick={() => consolidateFunds(p)}>
					Run fails with “no single note covers total”? Consolidate fragmented funds →
				</button>
			{:else if tab === 'subscriptions' && selSub}
				{@const s = selSub}
				<div class="detail-head">
					<AccountAvatar seed={s.label} size={40} />
					<div class="min-w-0 flex-1">
						<h2 class="truncate font-heading text-lg font-semibold">{s.label}</h2>
						<p class="text-xs text-muted-foreground">{cadenceLabel(s.cadence, s.interval_days)} · next {fmtDate(s.next_run_unix)}</p>
					</div>
				</div>
				<div class="kv">
					<div><dt>Amount</dt><dd class="font-mono">{scale(s.amount, s.asset)} {s.asset}</dd></div>
					<div><dt>Merchant</dt><dd class="font-mono">{truncate(s.code, 8, 6)}</dd></div>
					{#if s.auditor}<div><dt>Auditor</dt><dd class="font-mono">{truncate(s.auditor, 6, 4)}</dd></div>{/if}
					<div><dt>Ends</dt><dd>{s.end_unix ? fmtDate(s.end_unix) : 'No end'}</dd></div>
				</div>
				<div class="act">
					<Button class="flex-1" onclick={() => ask('Charge subscription', `Pay ${scale(s.amount, s.asset)} ${s.asset} now?`, () => runSub(s))}>Pay now</Button>
					<Button variant="outline" onclick={() => sEdit(s)}>Edit</Button>
					<Button variant="outline" onclick={() => toggleEnabled('sub', s.id, !s.enabled)}>{s.enabled ? 'Pause' : 'Resume'}</Button>
					<Button variant="ghost" onclick={() => ask('Delete subscription', `Delete "${s.label}"?`, async () => del('sub', s.id))}>Delete</Button>
				</div>
			{:else if tab === 'channels' && selChannel}
				{@const c = selChannel}
				{@const perTotal = c.amount_per_period > 0 ? Math.round(c.cap / c.amount_per_period) : 0}
				{@const perElapsed = c.amount_per_period > 0 ? Math.round(c.drawn_so_far / c.amount_per_period) : 0}
				<div class="detail-head">
					<AccountAvatar seed={`channel${c.id}`} size={40} />
					<div class="min-w-0 flex-1">
						<h2 class="font-heading text-lg font-semibold">Channel #{padId(c.id)}</h2>
						<p class="text-xs text-muted-foreground">{c.is_merchant ? 'You charge (merchant)' : 'You pay (subscriber)'} · {c.asset}</p>
					</div>
					{#if c.status === 'closed'}<Badge variant="secondary">Closed</Badge>{:else if c.expiry_passed}<Badge variant="destructive">Expired</Badge>{:else}<Badge>Open</Badge>{/if}
				</div>
				<p class="role-note">
					{#if c.is_subscriber}
						You locked {scale(c.cap, c.asset)} {c.asset} ({perTotal} periods). The merchant collects {scale(c.amount_per_period, c.asset)} per elapsed period; reclaim the rest after expiry.
					{:else}
						You may collect {scale(c.amount_per_period, c.asset)} {c.asset} per elapsed period, up to {scale(c.cap, c.asset)} {c.asset} total, until expiry.
					{/if}
				</p>

				{#if c.is_subscriber && c.status === 'open'}
					<div class="escrow-share">
						<div class="qr-wrap"><Qr data={channelShareCode(c)} size={108} themed /></div>
						<div class="min-w-0">
							<span class="share-l">Give the merchant this channel to collect</span>
							<div class="escrow-id">#{padId(c.id)}</div>
							<button type="button" class="linkbtn" onclick={() => { navigator.clipboard.writeText(channelShareCode(c)); toast.success('Channel code copied'); }}>Copy channel code</button>
						</div>
					</div>
				{/if}

				<div class="bar-wrap">
					<div class="bar"><div class="bar-fill" style="width:{c.cap > 0 ? Math.round((c.drawn_so_far / c.cap) * 100) : 0}%"></div></div>
					<span class="text-xs text-muted-foreground">{perElapsed} of {perTotal} periods · {c.is_merchant ? 'collectible' : 'drawn'} {scale(c.drawn_so_far, c.asset)} / {scale(c.cap, c.asset)} {c.asset}</span>
				</div>
				<div class="kv">
					<div><dt>Per period</dt><dd class="font-mono">{scale(c.amount_per_period, c.asset)} {c.asset}</dd></div>
					<div><dt>{c.is_merchant ? 'Collectible now' : 'Drawn so far'}</dt><dd class="font-mono">{scale(c.drawn_so_far, c.asset)} {c.asset}</dd></div>
					<div><dt>Expires</dt><dd>{c.expiry_passed ? 'expired' : 'expires'} {fmtDate(c.expiry_unix)}</dd></div>
				</div>
				<div class="act">
					{#if c.is_merchant && c.status === 'open'}
						<Button class="flex-1" disabled={!c.closeable} onclick={() => ask('Collect channel', `Collect ${scale(c.drawn_so_far, c.asset)} ${c.asset} and close channel #${padId(c.id)}?`, () => closeCh(c))}>Collect &amp; close</Button>
					{/if}
					{#if c.is_subscriber && c.status === 'open'}
						<Button class="flex-1" disabled={!c.reclaimable} onclick={() => ask('Reclaim channel', `Reclaim the unused balance of channel #${padId(c.id)}?`, () => reclaimCh(c))}>Reclaim unused</Button>
						{#if !c.reclaimable}<span class="act-hint">Reclaim opens after expiry ({fmtDate(c.expiry_unix)}).</span>{/if}
					{/if}
				</div>
			{:else if tab === 'escrow' && selEscrow}
				{@const e = selEscrow}
				<div class="detail-head">
					<AccountAvatar seed={`escrow${e.id}`} size={40} />
					<div class="min-w-0 flex-1">
						<h2 class="font-heading text-lg font-semibold">Escrow #{padId(e.id)}</h2>
						<p class="text-xs text-muted-foreground">{e.mode === 'all_or_nothing' ? 'All or nothing' : 'Keep what you raise'}</p>
					</div>
				</div>
				{#if e.is_payee}
					<div class="escrow-share">
						<div class="qr-wrap"><Qr data={escrowShareCode(e)} size={120} themed /></div>
						<div class="min-w-0">
							<span class="share-l">Share to receive contributions</span>
							<div class="escrow-id">#{padId(e.id)}</div>
							<button type="button" class="linkbtn" onclick={() => { navigator.clipboard.writeText(escrowShareCode(e)); toast.success('Escrow code copied'); }}>Copy escrow code</button>
						</div>
					</div>
				{/if}
				<div class="bar-wrap">
					{#if e.raised !== null}
						<div class="bar"><div class="bar-fill" style="width:{e.target > 0 ? Math.round((e.raised / e.target) * 100) : 0}%"></div></div>
						<span class="text-xs text-muted-foreground">Raised {scale(e.raised, e.asset)} / {scale(e.target, e.asset)} {e.asset}</span>
					{:else}
						<div class="bar hatch"></div>
						<span class="text-xs text-muted-foreground">Raised total is payee-only (hidden)</span>
					{/if}
				</div>
				<div class="kv">
					<div><dt>Contributions</dt><dd>{e.n_contrib}</dd></div>
					<div><dt>Deadline</dt><dd>{e.deadline_passed ? 'ended' : 'ends'} {fmtDate(e.deadline_unix)}</dd></div>
					<div><dt>Status</dt><dd class="capitalize">{e.status}</dd></div>
				</div>
				{#if e.my_contributions.length}
					<div class="sub-h">My contributions</div>
					{#each e.my_contributions as mc (mc.index)}
						<div class="payee">
							<span class="flex-1 font-mono text-xs">#{mc.index} · {scale(mc.amount, e.asset)} {e.asset}</span>
							{#if e.refundable}<Button variant="ghost" size="sm" onclick={() => ask('Refund', `Refund contribution #${mc.index}?`, () => refund(e, mc.index))}>Refund</Button>{/if}
						</div>
					{/each}
				{/if}
				<div class="act">
					{#if e.releasable}<Button class="flex-1" onclick={() => ask('Release escrow', `Release escrow #${e.id} to the payee?`, () => release(e))}>Release</Button>{/if}
				</div>
			{:else}
				<div class="grid h-full place-items-center text-sm text-muted-foreground">Select a schedule.</div>
			{/if}
		</aside>
	</div>
	{/if}
</div>

<!-- Confirm -->
<AlertDialog.Root open={!!confirm} onOpenChange={(o) => !o && (confirm = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{confirm?.title}</AlertDialog.Title>
			<AlertDialog.Description>{confirm?.body}</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={() => { const c = confirm; confirm = null; c?.run(); }}>Confirm</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>


<ProvingOverlay open={proving} title={provingTitle} />

<style>
	.hub {
		display: flex;
		flex-direction: column;
		gap: 16px;
		height: 100%;
		overflow: hidden;
		padding: 20px 32px 24px;
	}
	.topbar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
	}
	.actions {
		display: flex;
		gap: 8px;
	}
	:global(.pilltab) {
		border-radius: 9999px !important;
		font-size: 0.8125rem;
		color: var(--muted-foreground);
		transition: color 0.15s ease, background 0.15s ease;
	}
	:global(.pilltab:hover) {
		color: var(--foreground);
	}
	:global(.pilltab[data-state='active']) {
		background: var(--primary) !important;
		color: var(--primary-foreground) !important;
		box-shadow: none !important;
	}
	.cols {
		display: grid;
		grid-template-columns: 300px minmax(0, 1fr) 340px;
		gap: 18px;
		flex: 1;
		min-height: 0;
	}
	/* Narrower than the 3-column layout: stack list · calendar · detail and let the
	   page scroll, so the detail panel's actions (Run / Edit / Pause / Delete / keeper)
	   are always reachable. */
	@media (max-width: 1180px) {
		.hub {
			overflow-y: auto;
		}
		.cols {
			grid-template-columns: 1fr;
			flex: none;
		}
		.list,
		.detail {
			max-height: 420px;
		}
		.cal-card {
			min-height: 360px;
		}
	}
	.card {
		border: 1px solid var(--border);
		border-radius: var(--radius-3xl);
		background: var(--card);
		/* backdrop-filter: blur(16px);
		-webkit-backdrop-filter: blur(16px);
		box-shadow:
			0 1px 0 0 color-mix(in oklch, white 4%, transparent) inset,
			0 8px 24px -12px rgb(0 0 0 / 0.6); */
	}
	.list {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 14px;
		min-height: 0;
		overflow-y: auto;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: color-mix(in oklch, var(--card) 50%, transparent);
		transition: border-color 0.15s ease;
	}
	.row:hover {
		border-color: color-mix(in oklch, var(--primary) 30%, var(--border));
	}
	.row[data-active='true'] {
		border-color: var(--primary);
		box-shadow: inset 3px 0 0 var(--primary);
	}
	.center {
		display: flex;
		flex-direction: column;
		gap: 18px;
		min-height: 0;
	}
	.emphasis {
		border: 1px solid var(--border);
		border-radius: var(--radius-3xl);
		/* background: var(--card);
		backdrop-filter: blur(16px);
		-webkit-backdrop-filter: blur(16px); */
		box-shadow:
			0 1px 0 0 color-mix(in oklch, white 4%, transparent) inset,
			0 8px 24px -12px rgb(0 0 0 / 0.6);
		padding: 18px;
		border-color: color-mix(in oklch, var(--accent) 25%, var(--border) 0%);
		background: color-mix(in oklch, var(--accent) 90%, var(--card) 0%);
	}
	.emph-row {
		display: flex;
		justify-content: space-between;
	}
	.muted-label {
		font-size: 0.8125rem;
		color: var(--background);
	}
	.hero {
		font-family: var(--font-heading);
		font-weight: 600;
		font-size: clamp(1.75rem, 3vw, 2.5rem);
		line-height: 1.05;
		font-variant-numeric: tabular-nums;
		margin-top: 4px;
		color: var(--background);

	}
	.hero-unit {
		font-size: 1rem;
		color: var(--background);
	}
	.stats {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 10px;
		margin-top: 14px;
	}
	.stat {
		display: flex;
		flex-direction: column;
		color: var(--background);
	}
	.stat-n {
		font-family: var(--font-heading);
		font-size: 1.25rem;
		font-weight: 600;
		font-variant-numeric: tabular-nums;

	}
	.stat-l {
		font-size: 0.75rem;
		color: var(--background);
	}
	.cal-card {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
		padding: 16px;
	}
	.detail {
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 18px;
		min-height: 0;
		overflow-y: auto;
	}
	.detail-head {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.kv {
		display: flex;
		flex-direction: column;
		gap: 7px;
	}
	.kv div {
		display: flex;
		align-items: center;
		justify-content: space-between;
		font-size: 0.8125rem;
	}
	.kv dt {
		color: var(--muted-foreground);
	}
	.sub-h {
		font-size: 0.6875rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--muted-foreground);
		margin-top: 4px;
	}
	.payees {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.payee {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.bar-wrap {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.bar {
		height: 12px;
		border-radius: 9999px;
		overflow: hidden;
		background: var(--muted);
	}
	.bar.hatch {
		background: repeating-linear-gradient(
			45deg,
			var(--muted),
			var(--muted) 6px,
			color-mix(in oklch, var(--primary) 8%, transparent) 6px,
			color-mix(in oklch, var(--primary) 8%, transparent) 12px
		);
	}
	.bar-fill {
		height: 100%;
		border-radius: 9999px;
		background: var(--primary);
		transition: width 0.5s cubic-bezier(0.22, 1, 0.36, 1);
	}
	.act {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		margin-top: auto;
		padding-top: 8px;
	}
	.del {
		display: grid;
		place-items: center;
		width: 32px;
		height: 32px;
		flex-shrink: 0;
		border-radius: var(--radius-md);
		color: var(--muted-foreground);
	}
	.del:hover {
		color: var(--destructive);
		background: color-mix(in oklch, var(--destructive) 12%, transparent);
	}
	/* Inline create composer */
	.composer {
		display: flex;
		flex-direction: column;
		gap: 14px;
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding: 24px 28px;
	}
	.comp-title {
		font-family: var(--font-heading);
		font-size: 1.25rem;
		font-weight: 600;
	}
	.payee-table {
		display: flex;
		flex-direction: column;
		gap: 8px;
		max-width: 720px;
	}
	.payee-edit {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	/* Two-column composer (no-scroll: details+schedule left, payees right) */
	.compose2 {
		display: grid;
		grid-template-columns: minmax(300px, 360px) minmax(0, 1fr);
		gap: 24px;
		flex: 1;
		min-height: 0;
	}
	.col-left {
		display: flex;
		flex-direction: column;
		gap: 12px;
		min-width: 0;
	}
	.col-right {
		display: flex;
		flex-direction: column;
		gap: 12px;
		min-height: 0;
		min-width: 0;
	}
	.field-sep {
		height: 1px;
		background: var(--border);
		margin: 2px 0;
	}
	.two {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}
	.payee-scroll {
		display: flex;
		flex-direction: column;
		gap: 10px;
		min-height: 0;
		overflow-y: auto;
		padding-right: 4px;
	}
	@media (max-width: 880px) {
		.compose2 {
			grid-template-columns: 1fr;
		}
	}
	.opt {
		font-size: 0.6875rem;
		font-weight: 400;
		color: var(--muted-foreground);
		margin-left: 4px;
	}
	.consolidate-hint {
		text-align: left;
		font-size: 0.6875rem;
		line-height: 1.4;
		color: var(--muted-foreground);
		padding-top: 6px;
	}
	.consolidate-hint:hover {
		color: var(--primary);
	}
	.role-note {
		font-size: 0.75rem;
		line-height: 1.45;
		color: var(--muted-foreground);
	}
	.act-hint {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
		align-self: center;
	}
	.date-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	:global(.date-btn) {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		padding: 8px 12px;
		font-size: 0.875rem;
		text-align: left;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		background: var(--background);
		color: var(--foreground);
		transition: border-color 0.12s ease;
	}
	:global(.date-btn:hover) {
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
	:global(.date-btn [data-empty='true']) {
		color: var(--muted-foreground);
	}
	.hint {
		font-size: 0.75rem;
		color: var(--muted-foreground);
		line-height: 1.4;
	}
	.linkbtn {
		color: var(--primary);
		text-decoration: underline;
		text-underline-offset: 2px;
		font-size: 0.75rem;
	}
	.token-tabs {
		display: flex;
		align-items: center;
		gap: 6px;
		flex-wrap: wrap;
	}
	.ttab {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 5px 12px;
		font-size: 0.8125rem;
		border: 1px solid var(--border);
		border-radius: 9999px;
		background: color-mix(in oklch, var(--card) 50%, transparent);
		color: var(--muted-foreground);
		transition: border-color 0.15s ease, color 0.15s ease;
	}
	.ttab:hover {
		color: var(--foreground);
	}
	.ttab[data-active='true'] {
		border-color: var(--primary);
		background: color-mix(in oklch, var(--primary) 14%, transparent);
		color: var(--foreground);
	}
	.ttab-n {
		display: grid;
		place-items: center;
		min-width: 16px;
		height: 16px;
		padding: 0 4px;
		font-size: 0.625rem;
		font-weight: 600;
		border-radius: 9999px;
		background: var(--primary);
		color: var(--primary-foreground);
	}
	/* Escrow/channel forms shown inline in the detail panel */
	.detail-form-title {
		font-family: var(--font-heading);
		font-size: 1.0625rem;
		font-weight: 600;
	}
	.detail-form {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.escrow-share {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: color-mix(in oklch, var(--card) 50%, transparent);
	}
	.qr-wrap {
		flex-shrink: 0;
		border-radius: var(--radius-md);
		overflow: hidden;
		line-height: 0;
	}
	.share-l {
		font-size: 0.6875rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--muted-foreground);
	}
	.escrow-id {
		font-family: var(--font-heading);
		font-size: 1.5rem;
		font-weight: 600;
		font-variant-numeric: tabular-nums;
		line-height: 1.2;
	}
</style>
