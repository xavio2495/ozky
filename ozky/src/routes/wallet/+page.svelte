<script lang="ts">
	import { onMount } from 'svelte';
	import { get } from 'svelte/store';
	import { page } from '$app/stores';
	import { fly } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import * as Tabs from '$lib/components/ui/tabs';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import * as Select from '$lib/components/ui/select';
	import * as Field from '$lib/components/ui/field';
	import * as Chart from '$lib/components/ui/chart';
	import * as Alert from '$lib/components/ui/alert';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import AmountInput from '$lib/components/shared/AmountInput.svelte';
	import WalletAdvanced from '$lib/components/wallet/WalletAdvanced.svelte';
	import Qr from '$lib/components/shared/Qr.svelte';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import CheckIcon from '@lucide/svelte/icons/check';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import { api, errMessage, type AssetBalance, type PricePoint } from '$lib/api';
	import { ASSETS, toBaseUnits, assetByCode } from '$lib/assets';
	import { truncate, prettyAmount, toNumber } from '$lib/format';
	import { toast } from 'svelte-sonner';
	import { AreaChart } from 'layerchart';
	import { curveMonotoneX } from 'd3-shape';
	import InfoIcon from '@lucide/svelte/icons/info';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';

	const PAY_SLIPPAGE_BPS = 100;
	const fmtUsd = (n: number) =>
		`$${n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
	const decimalsOf = (code: string) =>
		wallet.balances.find((b) => b.code === code)?.decimals ?? assetByCode(code)?.decimals ?? 7;

	// ---- balances ----------------------------------------------------------
	type Holding = { code: string; bal?: AssetBalance; shielded: number; pub: number; usd: number };
	const holdings = $derived.by<Holding[]>(() => {
		const codes = new Set<string>([
			...ASSETS.map((a) => a.code),
			...wallet.balances.map((b) => b.code),
			...wallet.publicBalances.map((p) => p.code)
		]);
		const list: Holding[] = [];
		for (const code of codes) {
			const bal = wallet.balances.find((b) => b.code === code);
			const shielded = bal ? toNumber(bal.display) : 0;
			const pub = wallet.publicBalances
				.filter((p) => p.code === code)
				.reduce((s, p) => s + (Number(p.balance) || 0), 0);
			list.push({ code, bal, shielded, pub, usd: (shielded + pub) * wallet.priceOf(code) });
		}
		return list.sort((a, b) => b.usd - a.usd || b.shielded - a.shielded);
	});
	const holdingsUsd = $derived(holdings.reduce((s, h) => s + h.usd, 0));

	// ---- addresses ---------------------------------------------------------
	let shieldedAddr = $state('');
	let fundingAddr = $state('');
	onMount(async () => {
		try {
			[shieldedAddr, fundingAddr] = await Promise.all([api.receiveAddress(), api.fundingAddress()]);
		} catch {
			/* surfaced where needed */
		}
	});
	let qrLayer = $state<'shielded' | 'public'>('shielded');
	const qrValue = $derived(qrLayer === 'shielded' ? shieldedAddr : fundingAddr);

	// Copy BOTH the address text and the QR as a PNG image to the clipboard in one go.
	// A single ClipboardItem carries both representations; falls back to text-only.
	let copied = $state(false);
	async function copyQr() {
		if (!qrValue) return;
		try {
			const QRCode = (await import('qrcode')).default;
			const url = await QRCode.toDataURL(qrValue, { margin: 1, width: 320, color: { dark: '#0a0a0a', light: '#ffffff' } });
			const png = await (await fetch(url)).blob();
			await navigator.clipboard.write([
				new ClipboardItem({ 'text/plain': new Blob([qrValue], { type: 'text/plain' }), 'image/png': png })
			]);
		} catch {
			try {
				await navigator.clipboard.writeText(qrValue);
			} catch {
				toast.error('Could not copy');
				return;
			}
		}
		copied = true;
		toast.success('Address + QR copied');
		setTimeout(() => (copied = false), 1500);
	}

	// ---- price chart (driven by clicking a token row) ----------------------
	let chartCode = $state('XLM');
	$effect(() => {
		if (holdings.length && !holdings.some((h) => h.code === chartCode)) chartCode = holdings[0].code;
	});
	const ranges = [
		{ label: '24h', days: 1 },
		{ label: '7d', days: 7 },
		{ label: '30d', days: 30 }
	];
	let days = $state(7);
	let history = $state<{ t: Date; usd: number }[]>([]);
	const chartConfig: Chart.ChartConfig = { usd: { label: 'Price (USD)', color: 'var(--primary)' } };
	$effect(() => {
		const code = chartCode;
		const d = days;
		if (!code) return;
		let cancelled = false;
		(async () => {
			try {
				const pts: PricePoint[] = await api.priceHistory(code, d);
				if (!cancelled) history = pts.map((p) => ({ t: new Date(p.t), usd: p.usd }));
			} catch {
				if (!cancelled) history = [];
			}
		})();
		return () => {
			cancelled = true;
		};
	});

	// ---- shared helpers ----------------------------------------------------
	type Layer = 'shielded' | 'public';
	function recipientKind(addr: string): Layer | 'invalid' {
		const a = addr.trim();
		if (a.startsWith('ozky')) return 'shielded';
		if (a.startsWith('G') && a.length === 56) return 'public';
		return 'invalid';
	}
	const layerItems = [
		{ value: 'shielded', label: 'Shielded balance' },
		{ value: 'public', label: 'Public balance' }
	];
	const tokenItems = ASSETS.map((a) => ({ value: a.code, label: a.code }));

	let proving = $state(false);
	let provingTitle = $state('Working');
	// Active right-side tab — Advanced collapses the left column for a bigger canvas.
	// Honors a `?tab=` param so dashboard quick-actions (e.g. /wallet?tab=send) land here.
	const tabParam = get(page).url.searchParams.get('tab') ?? '';
	let activeTab = $state(['self', 'send', 'multi', 'advanced'].includes(tabParam) ? tabParam : 'send');
	let advRef = $state<WalletAdvanced>();

	async function refresh() {
		await wallet.refreshBalances();
		await wallet.refreshPublicBalances();
	}

	// ---- SELF tab ----------------------------------------------------------
	let selfDir = $state<'deposit' | 'withdraw'>('deposit');
	let selfToken = $state('XLM');
	let selfAmount = $state('');
	const selfBal = $derived(holdings.find((h) => h.code === selfToken));
	const selfCanSubmit = $derived(!!selfAmount);

	async function submitSelf() {
		const dec = decimalsOf(selfToken);
		let units: number;
		try {
			units = toBaseUnits(selfAmount, dec);
		} catch (e) {
			toast.error(errMessage(e));
			return;
		}
		proving = true;
		provingTitle = selfDir === 'deposit' ? 'Shielding deposit' : 'Withdrawing to self';
		let hash: string | undefined;
		if (selfDir === 'deposit') {
			hash = await runAction('Shielding deposit', () => api.deposit(selfToken, units), {
				success: () => 'Deposit shielded'
			});
			if (hash) wallet.log({ kind: 'deposit', label: `Deposited ${selfAmount} ${selfToken}`, hash });
		} else {
			hash = await runAction('Withdrawing', () => api.withdraw(selfToken, fundingAddr, units), {
				success: () => 'Withdrawn to your public balance'
			});
			if (hash) wallet.log({ kind: 'withdraw', label: `Withdrew ${selfAmount} ${selfToken} to self`, hash });
		}
		proving = false;
		if (hash) {
			selfAmount = '';
			await refresh();
		}
	}

	// ---- SEND tab ----------------------------------------------------------
	let sendToken = $state('XLM');
	let sendSource = $state<Layer>('shielded');
	let sendRecipient = $state('');
	let sendAmount = $state('');
	let sendConfirm = $state(false);
	const sendBal = $derived(holdings.find((h) => h.code === sendToken));
	const sendRecvKind = $derived(recipientKind(sendRecipient));
	// All four source × recipient combos are now wired to a backend command.
	const sendSupported = $derived(sendRecvKind !== 'invalid');
	const sendMaxRaw = $derived(sendSource === 'shielded' ? sendBal?.bal?.raw : undefined);

	function reviewSend() {
		try {
			toBaseUnits(sendAmount, decimalsOf(sendToken));
		} catch (e) {
			toast.error(errMessage(e));
			return;
		}
		if (sendRecvKind === 'invalid') {
			toast.error('Paste an ozky… or G… recipient address');
			return;
		}
		sendConfirm = true;
	}
	async function submitSend() {
		sendConfirm = false;
		const dec = decimalsOf(sendToken);
		const units = toBaseUnits(sendAmount, dec);
		const to = sendRecipient.trim();
		// Route by source × recipient layer.
		// shielded→shielded: send · shielded→public: withdraw
		// public→public: publicSend · public→shielded: deposit-then-send
		let call: () => Promise<string>;
		let kind: 'send' | 'withdraw' | 'deposit';
		if (sendSource === 'shielded') {
			call = sendRecvKind === 'public' ? () => api.withdraw(sendToken, to, units) : () => api.send(sendToken, to, units);
			kind = sendRecvKind === 'public' ? 'withdraw' : 'send';
			provingTitle = sendRecvKind === 'public' ? 'Withdrawing' : 'Sending payment';
		} else {
			call = sendRecvKind === 'public' ? () => api.publicSend(sendToken, to, units) : () => api.publicToShielded(sendToken, to, units);
			kind = sendRecvKind === 'public' ? 'withdraw' : 'deposit';
			provingTitle = sendRecvKind === 'public' ? 'Sending public payment' : 'Shielding then sending';
		}
		proving = true;
		const hash = await runAction(provingTitle, call, { success: () => 'Sent' });
		proving = false;
		if (hash) {
			wallet.log({ kind, label: `Sent ${sendAmount} ${sendToken}`, detail: truncate(to), hash });
			sendAmount = '';
			sendRecipient = '';
			await refresh();
		}
	}

	// ---- MULTI-SEND tab ----------------------------------------------------
	type MsRow = { recipient: string; recvToken: string; amount: string };
	let msSource = $state<Layer>('shielded');
	let msPayToken = $state('XLM');
	let msRows = $state<MsRow[]>([{ recipient: '', recvToken: 'XLM', amount: '' }]);
	let msConfirm = $state(false);
	function msAdd() {
		if (msRows.length < 5) msRows = [...msRows, { recipient: '', recvToken: msPayToken, amount: '' }];
	}
	function msRemove(i: number) {
		msRows = msRows.filter((_, idx) => idx !== i);
	}
	// Shielded source → one private split/multi_send to ozky recipients.
	// Public source → a sequence of ordinary payments to each recipient (G or ozky).
	const msValid = $derived(
		msRows.filter((r) => {
			const k = recipientKind(r.recipient);
			if (!r.amount.trim()) return false;
			return msSource === 'shielded' ? k === 'shielded' : k !== 'invalid';
		})
	);
	const msSupported = $derived(msValid.length === msRows.length && msRows.length > 0);

	async function submitMulti() {
		msConfirm = false;
		proving = true;
		provingTitle = 'Sending to recipients';
		let hash: string | undefined;
		if (msSource === 'shielded') {
			const recipients = msRows.map((r) => ({
				recipient: r.recipient.trim(),
				amount: toBaseUnits(r.amount, decimalsOf(r.recvToken)),
				recv_asset: r.recvToken === msPayToken ? undefined : r.recvToken
			}));
			const anyCross = recipients.some((r) => r.recv_asset);
			hash = await runAction(
				'Sending to recipients',
				() =>
					anyCross
						? api.multiSend(msPayToken, recipients).then((hs) => hs[0])
						: api.split(
								msPayToken,
								recipients.map(({ recipient, amount }) => ({ recipient, amount }))
							),
				{ success: () => 'Sent to recipients' }
			);
		} else {
			// Public source: pay each recipient in turn with the pay token.
			let last: string | undefined;
			for (const r of msRows) {
				const to = r.recipient.trim();
				const units = toBaseUnits(r.amount, decimalsOf(msPayToken));
				last = await runAction(
					`Paying ${truncate(to)}`,
					() => (recipientKind(to) === 'public' ? api.publicSend(msPayToken, to, units) : api.publicToShielded(msPayToken, to, units)),
					{ success: () => 'Paid' }
				);
				if (!last) break;
			}
			hash = last;
		}
		proving = false;
		if (hash) {
			wallet.log({ kind: 'split', label: `Multi-send ${msPayToken}`, detail: `${msRows.length} recipients`, hash });
			msRows = [{ recipient: '', recvToken: msPayToken, amount: '' }];
			await refresh();
		}
	}
</script>

<div class="wallet" class:adv={activeTab === 'advanced'}>
	<!-- LEFT -->
	<div class="left" in:fly={{ y: 12, duration: 320, easing: cubicOut }}>
		<!-- QR (public / shielded) -->
		<section class="card qr-card">
			<ToggleGroup.Root type="single" bind:value={qrLayer} class="grid grid-cols-2">
				<ToggleGroup.Item value="shielded" class="text-xs">Shielded</ToggleGroup.Item>
				<ToggleGroup.Item value="public" class="text-xs">Public</ToggleGroup.Item>
			</ToggleGroup.Root>
			<div class="qr-body">
				{#if qrValue}
					<Qr data={qrValue} size={190} themed />
				{:else}
					<div class="qr-skel">Loading…</div>
				{/if}
			</div>
			<div class="qr-foot">
				<span class="text-xs text-muted-foreground">
					{qrLayer === 'shielded' ? 'Private ozky code' : 'Public funding address'}
				</span>
				<Button variant="outline" size="icon" onclick={copyQr} aria-label="Copy address and QR" title="Copy address + QR image">
					{#if copied}<CheckIcon class="size-4 text-primary" />{:else}<CopyIcon class="size-4" />{/if}
				</Button>
			</div>
		</section>

		<!-- Token balances (informational) -->
		<section class="card tokens">
			<div class="row-between mb-2">
				<h3 class="card-title">Balances</h3>
				<span class="font-mono text-sm tabular-nums text-muted-foreground">{fmtUsd(holdingsUsd)}</span>
			</div>
			<div class="token-list">
				{#each holdings as h (h.code)}
					{@const meta = assetByCode(h.code)}
					<button class="token" data-active={h.code === chartCode} onclick={() => (chartCode = h.code)}>
						<span
							class="glyph"
							style="color:{meta?.accent ?? 'var(--primary)'}; background:color-mix(in oklch, {meta?.accent ??
								'var(--primary)'} 16%, var(--card));"
						>
							{h.code.slice(0, 2)}
						</span>
						<div class="min-w-0 flex-1 text-left">
							<div class="text-sm font-medium">{h.code}</div>
							<div class="truncate text-xs text-muted-foreground">{meta?.name ?? h.code}</div>
						</div>
						<div class="text-right text-xs tabular-nums">
							<div class="font-mono"><span class="text-primary">◆</span> {prettyAmount(h.bal?.display ?? '0')}</div>
							<div class="text-muted-foreground">○ {prettyAmount(String(h.pub))}</div>
						</div>
					</button>
				{/each}
			</div>
		</section>

		<!-- Price chart -->
		<section class="card chart-card">
			<div class="row-between mb-2">
				<h3 class="card-title">{chartCode} price</h3>
				<div class="flex gap-1">
					{#each ranges as r (r.days)}
						<Button variant={days === r.days ? 'secondary' : 'ghost'} size="sm" class="h-7 px-2.5 text-xs" onclick={() => (days = r.days)}>
							{r.label}
						</Button>
					{/each}
				</div>
			</div>
			{#if history.length === 0}
				<div class="grid flex-1 place-items-center text-sm text-muted-foreground">No price data.</div>
			{:else}
				<Chart.Container config={chartConfig} class="aspect-auto h-full w-full flex-1">
					<AreaChart
						data={history}
						x="t"
						y="usd"
						axis="x"
						series={[{ key: 'usd', label: 'USD', color: 'var(--primary)' }]}
						props={{ area: { curve: curveMonotoneX, 'fill-opacity': 0.15, line: { class: 'stroke-2' } } }}
					>
						{#snippet tooltip()}
							<Chart.Tooltip />
						{/snippet}
					</AreaChart>
				</Chart.Container>
			{/if}
		</section>
	</div>

	<!-- RIGHT: action tabs -->
	<section class="card right" in:fly={{ x: 20, duration: 360, delay: 80, easing: cubicOut }}>
		<Tabs.Root bind:value={activeTab} class="flex h-full flex-col">
			<div class="tabrow">
				<Tabs.List class="pillnav grid w-fit grid-cols-4 gap-1 rounded-full border border-border bg-card/50 p-1">
					<Tabs.Trigger value="self" class="pilltab">Self</Tabs.Trigger>
					<Tabs.Trigger value="send" class="pilltab">Send</Tabs.Trigger>
					<Tabs.Trigger value="multi" class="pilltab">Multi-send</Tabs.Trigger>
					<Tabs.Trigger value="advanced" class="pilltab">Advanced</Tabs.Trigger>
				</Tabs.List>
				{#if activeTab === 'advanced'}
					<div class="adv-tools">
						<Button variant="outline" size="sm" onclick={() => advRef?.add('source')}>+ Source</Button>
						<Button variant="outline" size="sm" onclick={() => advRef?.add('transform')}>+ Transform</Button>
						<Button variant="outline" size="sm" onclick={() => advRef?.add('destination')}>+ Destination</Button>
						<Button size="sm" onclick={() => advRef?.preview()}>Preview plan</Button>
					</div>
				{/if}
			</div>

			<!-- SELF: deposit / withdraw between your own public & shielded -->
			<Tabs.Content value="self" class="wform">
				<ToggleGroup.Root type="single" bind:value={selfDir} class="grid grid-cols-2">
					<ToggleGroup.Item value="deposit" class="text-xs">Deposit (public → shielded)</ToggleGroup.Item>
					<ToggleGroup.Item value="withdraw" class="text-xs">Withdraw (shielded → public)</ToggleGroup.Item>
				</ToggleGroup.Root>
				<Field.Group class="mt-3">
					<Field.Field>
						<Field.Label>Token</Field.Label>
						<Select.Root type="single" bind:value={selfToken}>
							<Select.Trigger class="bg-popover">{selfToken}</Select.Trigger>
							<Select.Content class="bg-popover">
								{#each tokenItems as t (t.value)}<Select.Item value={t.value}>{t.label}</Select.Item>{/each}
							</Select.Content>
						</Select.Root>
					</Field.Field>
					<Field.Field>
						<Field.Label>Amount</Field.Label>
						<AmountInput bind:value={selfAmount} code={selfToken} decimals={decimalsOf(selfToken)} max={selfDir === 'withdraw' ? selfBal?.bal?.raw : undefined} />
						<Field.Description>
							{#if selfDir === 'deposit'}Shields from your public balance ({prettyAmount(String(selfBal?.pub ?? 0))} {selfToken}).
							{:else}Unshields to your own public address. Shielded: {selfBal?.bal?.display ?? '0'} {selfToken}.{/if}
						</Field.Description>
					</Field.Field>
				</Field.Group>
				<Button class="mt-auto w-full" onclick={submitSelf} disabled={!selfCanSubmit}>
					{selfDir === 'deposit' ? 'Shield deposit' : 'Withdraw to self'}
				</Button>
			</Tabs.Content>

			<!-- SEND: token + source dropdowns; recipient auto-detected -->
			<Tabs.Content value="send" class="wform">
				<Field.Group>
					<div class="grid grid-cols-2 gap-3">
						<Field.Field>
							<Field.Label>Token</Field.Label>
							<Select.Root type="single" bind:value={sendToken}>
								<Select.Trigger class="bg-popover">{sendToken}</Select.Trigger>
								<Select.Content class="bg-popover">
									{#each tokenItems as t (t.value)}<Select.Item value={t.value}>{t.label}</Select.Item>{/each}
								</Select.Content>
							</Select.Root>
						</Field.Field>
						<Field.Field>
							<Field.Label>Spend from</Field.Label>
							<Select.Root type="single" bind:value={sendSource}>
								<Select.Trigger class="bg-popover">{sendSource === 'shielded' ? 'Shielded' : 'Public'}</Select.Trigger>
								<Select.Content class="bg-popover">
									{#each layerItems as l (l.value)}<Select.Item value={l.value}>{l.label}</Select.Item>{/each}
								</Select.Content>
							</Select.Root>
						</Field.Field>
					</div>
					<Field.Field>
						<Field.Label for="send-rcpt">Recipient</Field.Label>
						<Input id="send-rcpt" bind:value={sendRecipient} placeholder="ozky… or G…" class="font-mono" />
						<Field.Description>
							{#if sendRecvKind === 'shielded'}<span class="text-primary">Shielded</span> recipient — they receive privately.
							{:else if sendRecvKind === 'public'}<span>Public</span> recipient — this amount becomes public.
							{:else}Paste an ozky… (shielded) or G… (public) address.{/if}
						</Field.Description>
					</Field.Field>
					<Field.Field>
						<Field.Label>Amount</Field.Label>
						<AmountInput bind:value={sendAmount} code={sendToken} decimals={decimalsOf(sendToken)} max={sendMaxRaw} />
						<Field.Description>
							{sendSource === 'shielded' ? 'Shielded' : 'Public'}: {sendSource === 'shielded' ? (sendBal?.bal?.display ?? '0') : prettyAmount(String(sendBal?.pub ?? 0))} {sendToken}
						</Field.Description>
					</Field.Field>
				</Field.Group>
				{#if sendSource === 'public' && sendRecvKind === 'public'}
					<Alert.Root class="mt-3">
						<InfoIcon />
						<Alert.Description class="text-xs">
							Public → public is an ordinary Stellar payment — <b>not private</b>. Both ends are
							visible on-chain.
						</Alert.Description>
					</Alert.Root>
				{:else if sendSource === 'public' && sendRecvKind === 'shielded'}
					<Alert.Root class="mt-3">
						<InfoIcon />
						<Alert.Description class="text-xs">
							Public → shielded runs as two steps: shield to your pool, then send privately. The
							deposit is visible; the send is private.
						</Alert.Description>
					</Alert.Root>
				{/if}
				<Button class="mt-auto w-full" onclick={reviewSend} disabled={!sendSupported || !sendAmount}>
					{sendRecvKind === 'public' ? 'Review withdrawal' : 'Review payment'}
				</Button>
			</Tabs.Content>

			<!-- MULTI-SEND: one source, many shielded recipients -->
			<Tabs.Content value="multi" class="wform">
				<div class="grid grid-cols-2 gap-3">
					<Field.Field>
						<Field.Label>Pay token</Field.Label>
						<Select.Root type="single" bind:value={msPayToken}>
							<Select.Trigger class="bg-popover">{msPayToken}</Select.Trigger>
							<Select.Content class="bg-popover">
								{#each tokenItems as t (t.value)}<Select.Item value={t.value}>{t.label}</Select.Item>{/each}
							</Select.Content>
						</Select.Root>
					</Field.Field>
					<Field.Field>
						<Field.Label>Spend from</Field.Label>
						<Select.Root type="single" bind:value={msSource}>
							<Select.Trigger class="bg-popover">{msSource === 'shielded' ? 'Shielded' : 'Public'}</Select.Trigger>
							<Select.Content class="bg-popover">
								{#each layerItems as l (l.value)}<Select.Item value={l.value}>{l.label}</Select.Item>{/each}
							</Select.Content>
						</Select.Root>
					</Field.Field>
				</div>
				<div class="ms-head">
					<span class="ms-rcpt">Recipient</span>
					<span class="ms-tok">Receives</span>
					<span class="ms-amt">Amount</span>
				</div>
				<div class="ms-rows">
					{#each msRows as row, i (i)}
						<div class="ms-row">
							<Input bind:value={row.recipient} placeholder="ozkGAy.../K..." class="font-mono ms-rcpt" />
							<Select.Root type="single" bind:value={row.recvToken} disabled={msSource === 'public'}>
								<Select.Trigger class="bg-popover ms-tok">{msSource === 'public' ? msPayToken : `${row.recvToken}`}</Select.Trigger>
								<Select.Content class="bg-popover">
									{#each tokenItems as t (t.value)}<Select.Item value={t.value}>{t.label}</Select.Item>{/each}
								</Select.Content>
							</Select.Root>
							<Input bind:value={row.amount} placeholder="000.00" inputmode="decimal" class="font-mono ms-amt" />
							{#if msRows.length > 1}
								<button class="ms-x" onclick={() => msRemove(i)} aria-label="Remove"><XIcon class="size-4" /></button>
							{/if}
						</div>
					{/each}
				</div>
				<Button variant="ghost" size="sm" class="w-fit" onclick={msAdd} disabled={msRows.length >= 5}>
					<PlusIcon data-icon="inline-start" /> Add recipient
				</Button>
				<Alert.Root class="mt-2">
					<InfoIcon />
					<Alert.Description class="text-xs">
						{#if msSource === 'shielded'}
							One private transaction to all recipients (ozky… codes). Per-row receive token swaps in-pool.
						{:else}
							Public source pays each recipient as a separate Stellar payment (per-row receive token is ignored).
						{/if}
					</Alert.Description>
				</Alert.Root>
				<Button class="mt-auto w-full" onclick={() => (msConfirm = true)} disabled={!msSupported || msValid.length === 0}>
					Review multi-send ({msValid.length})
				</Button>
			</Tabs.Content>

			<!-- ADVANCED: node-based flow builder (xyflow) -->
			<Tabs.Content value="advanced" class="wform">
				<WalletAdvanced bind:this={advRef} />
			</Tabs.Content>
		</Tabs.Root>
	</section>
</div>

<!-- Send confirm -->
<AlertDialog.Root bind:open={sendConfirm}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{sendRecvKind === 'public' ? 'Confirm withdrawal' : 'Confirm payment'}</AlertDialog.Title>
			<AlertDialog.Description>
				Send <b>{sendAmount} {sendToken}</b> from your {sendSource} balance to
				<span class="font-mono">{truncate(sendRecipient)}</span>?
				{#if sendRecvKind === 'public'} The destination and amount become public on-chain.{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={submitSend}>{sendRecvKind === 'public' ? 'Withdraw' : 'Send'}</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<!-- Multi-send confirm -->
<AlertDialog.Root bind:open={msConfirm}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Confirm multi-send</AlertDialog.Title>
			<AlertDialog.Description>
				Pay <b>{msValid.length}</b> recipient{msValid.length === 1 ? '' : 's'} from your shielded {msPayToken}
				balance in one private transaction?
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={submitMulti}>Send</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title={provingTitle} />

<style>
	.wallet {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1.28fr);
		gap: 24px;
		height: 100%;
		overflow: hidden;
		padding: 20px 32px 24px;
		transition: grid-template-columns 0.42s cubic-bezier(0.4, 0, 0.2, 1);
	}
	/* Advanced tab: animate the left column to zero width so the canvas expands full-screen. */
	.wallet.adv {
		grid-template-columns: 0fr minmax(0, 1fr);
		gap: 0;
		transition:
			grid-template-columns 0.42s cubic-bezier(0.4, 0, 0.2, 1),
			gap 0.42s cubic-bezier(0.4, 0, 0.2, 1);
	}
	.tabrow {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		flex-wrap: wrap;
	}
	.adv-tools {
		display: flex;
		gap: 6px;
		flex-wrap: wrap;
	}
	.left {
		min-width: 0;
		overflow: hidden;
		transition: opacity 0.28s ease;
	}
	.wallet.adv .left {
		opacity: 0;
		pointer-events: none;
	}
	@media (max-width: 1000px) {
		.wallet {
			grid-template-columns: 1fr;
			overflow-y: auto;
		}
	}
	/* Pill tabs — match the top navbar (rounded container + gold active pill). */
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
	.left {
		display: grid;
		grid-template-columns: 240px 1fr;
		grid-template-rows: auto minmax(0, 1fr);
		grid-template-areas: 'qr tokens' 'chart chart';
		gap: 18px;
		min-height: 0;
	}
	.qr-card {
		grid-area: qr;
		gap: 12px;
	}
	.tokens {
		grid-area: tokens;
	}
	.chart-card {
		grid-area: chart;
		min-height: 0;
	}
	.card {
		display: flex;
		flex-direction: column;
		padding: 18px;
		border: 1px solid var(--border);
		border-radius: var(--radius-3xl);
		background: var(--card);
		/* backdrop-filter: blur(16px);
		-webkit-backdrop-filter: blur(16px);
		box-shadow:
			0 1px 0 0 color-mix(in oklch, white 4%, transparent) inset,
			0 8px 24px -12px rgb(0 0 0 / 0.6); */
	}
	.qr-body {
		display: grid;
		place-items: center;
		flex: 1;
		padding: 6px 0;
	}
	.qr-skel {
		display: grid;
		place-items: center;
		width: 150px;
		height: 150px;
		border-radius: var(--radius-lg);
		background: var(--muted);
		color: var(--muted-foreground);
		font-size: 0.8125rem;
	}
	.qr-foot {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
	}
	.right {
		min-height: 0;
	}
	.row-between {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}
	.card-title {
		font-family: var(--font-heading);
		font-size: 0.9375rem;
		font-weight: 600;
	}
	.token-list {
		display: flex;
		flex-direction: column;
		gap: 6px;
		min-height: 0;
		overflow-y: auto;
	}
	.token {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 9px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: color-mix(in oklch, var(--card) 50%, transparent);
		transition: border-color 0.15s ease;
	}
	.token:hover {
		border-color: color-mix(in oklch, var(--primary) 30%, var(--border));
	}
	.token[data-active='true'] {
		border-color: var(--primary);
	}
	.glyph {
		display: grid;
		place-items: center;
		width: 34px;
		height: 34px;
		flex-shrink: 0;
		border-radius: 9999px;
		font-size: 0.6875rem;
		font-weight: 700;
	}
	/* tabs forms (class lands on the Tabs.Content child component → global) */
	:global(.wform) {
		display: flex;
		flex-direction: column;
		gap: 4px;
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding-top: 14px;
	}
	.ms-head {
		display: flex;
		align-items: center;
		gap: 6px;
		margin-top: 8px;
		padding: 0 2px;
		font-size: 0.6875rem;
		text-transform: uppercase;
		letter-spacing: 0.03em;
		color: var(--muted-foreground);
	}
	.ms-head .ms-rcpt,
	.ms-head .ms-tok,
	.ms-head .ms-amt {
		display: block;
	}
	.ms-rows {
		display: flex;
		flex-direction: column;
		gap: 8px;
		margin: 6px 0;
	}
	.ms-row {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	:global(.ms-rcpt) {
		flex: 1;
		min-width: 0;
	}
	:global(.ms-tok) {
		width: 90px;
		flex-shrink: 0;
	}
	:global(.ms-amt) {
		width: 200px;
		flex-shrink: 0;
	}
	.ms-x {
		display: grid;
		place-items: center;
		width: 30px;
		height: 30px;
		flex-shrink: 0;
		border-radius: var(--radius-md);
		color: var(--muted-foreground);
	}
	.ms-x:hover {
		color: var(--destructive);
		background: color-mix(in oklch, var(--destructive) 12%, transparent);
	}
</style>
