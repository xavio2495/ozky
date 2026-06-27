<script lang="ts">
	import { onMount } from 'svelte';
	import { fly } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import AssetSelect from '$lib/components/shared/AssetSelect.svelte';
	import AmountInput from '$lib/components/shared/AmountInput.svelte';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import * as Chart from '$lib/components/ui/chart';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { AreaChart } from 'layerchart';
	import { curveMonotoneX } from 'd3-shape';
	import { api, errMessage, type Spot, type PricePoint, type SwapQuote } from '$lib/api';
	import { wallet } from '$lib/wallet.svelte';
	import { ASSETS, toBaseUnits, assetByCode } from '$lib/assets';
	import { toast } from 'svelte-sonner';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ArrowDownRightIcon from '@lucide/svelte/icons/arrow-down-right';
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';

	// ---- markets -----------------------------------------------------------
	const ranges = [
		{ label: '24h', days: 1 },
		{ label: '7d', days: 7 },
		{ label: '30d', days: 30 },
		{ label: '90d', days: 90 }
	];
	let spots = $state<Spot[]>([]);
	let chartAsset = $state('EURC');
	let days = $state(7);
	let history = $state<{ t: Date; usd: number }[]>([]);
	let loadingChart = $state(false);
	const spot = $derived(spots.find((s) => s.code === chartAsset));
	const chartConfig: Chart.ChartConfig = { usd: { label: 'Price (USD)', color: 'var(--primary)' } };

	const fmtUsd = (n: number) =>
		n >= 1 ? n.toLocaleString('en-US', { style: 'currency', currency: 'USD' }) : `$${n.toFixed(4)}`;

	async function loadSpots() {
		try {
			spots = await api.assetPrices();
		} catch (e) {
			toast.error('Could not load prices', { description: errMessage(e) });
		}
	}
	async function loadHistory() {
		loadingChart = true;
		try {
			const pts: PricePoint[] = await api.priceHistory(chartAsset, days);
			history = pts.map((p) => ({ t: new Date(p.t), usd: p.usd }));
		} catch (e) {
			history = [];
			toast.error('Could not load chart', { description: errMessage(e) });
		} finally {
			loadingChart = false;
		}
	}

	onMount(loadSpots);
	$effect(() => {
		chartAsset;
		days;
		loadHistory();
	});

	// Clicking a market focuses the chart and (when it keeps a valid pair) targets the swap.
	function focusMarket(code: string) {
		chartAsset = code;
		if (code !== from) to = code;
	}

	// ---- swap --------------------------------------------------------------
	let from = $state('USDC');
	let to = $state('EURC');
	let amount = $state('');
	let slippagePct = $state('1');
	let quote = $state<SwapQuote | null>(null);
	let quoting = $state(false);
	let quoteErr = $state('');
	let confirmOpen = $state(false);
	let proving = $state(false);

	const bal = $derived(wallet.balances.find((b) => b.code === from));
	const toDecimals = $derived(assetByCode(to)?.decimals ?? 7);
	const slippageBps = $derived(Math.round((Number(slippagePct) || 0) * 100));
	const minReceived = $derived(
		quote ? Math.floor((quote.dest_amount * (10_000 - slippageBps)) / 10_000) : 0
	);
	const fmt = (units: number, decimals = toDecimals) =>
		(units / 10 ** decimals).toLocaleString('en-US', { maximumFractionDigits: decimals });
	const rate = $derived(
		quote && Number(amount) > 0 ? quote.dest_amount / 10 ** toDecimals / Number(amount) : 0
	);

	function flip() {
		[from, to] = [to, from];
		amount = '';
	}

	// Auto-quote (debounced) on pair / amount change.
	let quoteTimer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => {
		const f = from,
			t = to,
			a = amount;
		quote = null;
		quoteErr = '';
		clearTimeout(quoteTimer);
		if (f === t || !a.trim()) return;
		let units: number;
		try {
			units = toBaseUnits(a, bal?.decimals ?? 7);
		} catch {
			return;
		}
		quoting = true;
		quoteTimer = setTimeout(async () => {
			try {
				quote = await api.swapQuote(f, t, units);
			} catch (e) {
				quoteErr = errMessage(e);
			} finally {
				quoting = false;
			}
		}, 450);
	});

	function review() {
		if (from === to) return toast.error('Choose two different assets');
		try {
			toBaseUnits(amount, bal?.decimals ?? 7);
		} catch (e) {
			return toast.error(errMessage(e));
		}
		if (slippageBps <= 0 || slippageBps > 5000) return toast.error('Slippage must be between 0% and 50%');
		confirmOpen = true;
	}
	async function submit() {
		confirmOpen = false;
		const units = toBaseUnits(amount, bal?.decimals ?? 7);
		proving = true;
		try {
			const r = await api.swap(from, to, units, slippageBps);
			toast.success(`Swapped ${amount} ${from} → ${fmt(r.received)} ${to}`);
			wallet.log({
				kind: 'swap',
				label: `Swapped ${amount} ${from} → ${to}`,
				detail: `received ${fmt(r.received)} ${to}`,
				hash: r.tx_hash
			});
			amount = '';
			quote = null;
			await wallet.refreshBalances();
		} catch (e) {
			toast.error('Swap failed', { description: errMessage(e) });
		} finally {
			proving = false;
		}
	}
</script>

<div class="hub">
	<!-- ticker strip -->
	<div class="tickers">
		{#each ASSETS as a (a.code)}
			{@const s = spots.find((x) => x.code === a.code)}
			<button class="ticker" data-active={a.code === chartAsset} onclick={() => focusMarket(a.code)}>
				<div class="t-head">
					<span class="t-code">{a.code}</span>
					<span class="t-dot" style="background:{a.accent}"></span>
				</div>
				{#if s}
					<div class="t-row">
						<span class="t-price">{fmtUsd(s.usd)}</span>
						<span class="t-change" data-up={s.change_24h >= 0}>
							{#if s.change_24h >= 0}<ArrowUpRightIcon class="size-3" />{:else}<ArrowDownRightIcon class="size-3" />{/if}
							{Math.abs(s.change_24h).toFixed(2)}%
						</span>
					</div>
				{:else}
					<Skeleton class="mt-2 h-4 w-16" />
				{/if}
			</button>
		{/each}
	</div>

	<div class="cols" in:fly={{ y: 12, duration: 320, easing: cubicOut }}>
		<!-- LEFT: price chart -->
		<section class="card chart-card">
			<div class="chart-head">
				<div>
					<div class="chart-title">
						{chartAsset}
						{#if spot}
							<span class="chart-spot">{fmtUsd(spot.usd)}</span>
							<Badge variant={spot.change_24h >= 0 ? 'secondary' : 'destructive'} class="gap-1">
								{spot.change_24h >= 0 ? '+' : ''}{spot.change_24h.toFixed(2)}% 24h
							</Badge>
						{/if}
					</div>
					<p class="chart-sub">USD price · last {days === 1 ? '24 hours' : `${days} days`}</p>
				</div>
				<div class="ranges">
					{#each ranges as r (r.days)}
						<button class="range" data-active={days === r.days} onclick={() => (days = r.days)}>{r.label}</button>
					{/each}
				</div>
			</div>
			<div class="chart-body">
				{#if loadingChart && history.length === 0}
					<Skeleton class="h-full w-full rounded-lg" />
				{:else if history.length === 0}
					<div class="grid h-full place-items-center text-sm text-muted-foreground">No price data.</div>
				{:else}
					<Chart.Container config={chartConfig} class="h-full w-full">
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
			</div>
		</section>

		<!-- RIGHT: swap panel -->
		<aside class="card swap-card">
			<div class="swap-title"><ShieldCheckIcon class="size-4 text-primary" />Convert shielded assets</div>

			<div class="leg">
				<div class="leg-head"><span>From</span>{#if bal}<span class="leg-bal">{bal.display} {from}</span>{/if}</div>
				<AssetSelect bind:value={from} />
				<AmountInput bind:value={amount} code={from} decimals={bal?.decimals ?? 7} max={bal?.raw} />
			</div>

			<button class="flip" onclick={flip} aria-label="Flip assets"><ArrowDownIcon class="size-4" /></button>

			<div class="leg">
				<div class="leg-head"><span>To</span></div>
				<AssetSelect bind:value={to} />
				<div class="quote">
					{#if quoting}
						Pricing against the pool…
					{:else if quoteErr}
						<span class="text-destructive">{quoteErr}</span>
					{:else if quote && quote.dest_amount === 0}
						<span class="text-destructive">Pool can't fill this — thin {to} liquidity at these reserves.</span>
					{:else if quote}
						≈ <b>{fmt(quote.dest_amount)} {to}</b>
						<span class="quote-sub">min {fmt(minReceived)} after slippage{#if rate} · 1 {from} ≈ {rate.toLocaleString('en-US', { maximumFractionDigits: 4 })} {to}{/if}</span>
					{:else}
						Enter an amount to see a quote.
					{/if}
				</div>
			</div>

			<div class="slip">
				<span class="slip-l">Slippage tolerance</span>
				<div class="slip-in"><Input bind:value={slippagePct} type="number" step="0.1" class="w-20" /><span>%</span></div>
			</div>

			<Button class="w-full" onclick={review} disabled={!amount || from === to || !!quoteErr || quote?.dest_amount === 0}>Review swap</Button>
			<p class="note">An in-pool AMM trade — funds never leave the shielded pool. The trade amount and pair are visible on-chain; your identity stays hidden.</p>
		</aside>
	</div>
</div>

<AlertDialog.Root bind:open={confirmOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Confirm swap</AlertDialog.Title>
			<AlertDialog.Description>
				Swap <b>{amount} {from}</b> for at least <b>{fmt(minReceived)} {to}</b> (slippage {slippagePct}%)? This is a single in-pool transaction.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={submit}>Swap</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title="Swapping" />

<style>
	.hub {
		display: flex;
		flex-direction: column;
		gap: 16px;
		height: 100%;
		overflow: hidden;
		padding: 20px 32px 24px;
	}
	.tickers {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
		gap: 10px;
	}
	.ticker {
		padding: 12px 14px;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: color-mix(in oklch, var(--card) 50%, transparent);
		text-align: left;
		transition: border-color 0.15s ease, transform 0.15s ease;
	}
	.ticker:hover {
		transform: translateY(-2px);
		border-color: color-mix(in oklch, var(--primary) 30%, var(--border));
	}
	.ticker[data-active='true'] {
		border-color: var(--primary);
		background: color-mix(in oklch, var(--primary) 10%, transparent);
	}
	.t-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.t-code {
		font-weight: 600;
		font-size: 0.875rem;
	}
	.t-dot {
		width: 8px;
		height: 8px;
		border-radius: 9999px;
	}
	.t-row {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 8px;
		margin-top: 6px;
	}
	.t-price {
		font-family: var(--font-mono, monospace);
		font-size: 0.875rem;
		font-variant-numeric: tabular-nums;
	}
	.t-change {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		font-size: 0.6875rem;
		white-space: nowrap;
	}
	.t-change[data-up='true'] {
		color: oklch(0.72 0.15 150);
	}
	.t-change[data-up='false'] {
		color: var(--destructive);
	}
	.cols {
		display: grid;
		grid-template-columns: minmax(0, 1fr) 380px;
		gap: 18px;
		flex: 1;
		min-height: 0;
	}
	@media (max-width: 1080px) {
		.hub {
			overflow-y: auto;
		}
		.cols {
			grid-template-columns: 1fr;
			flex: none;
		}
		.chart-card {
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
	.chart-card {
		display: flex;
		flex-direction: column;
		gap: 14px;
		padding: 18px 20px;
		min-height: 0;
	}
	.chart-head {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: 16px;
	}
	.chart-title {
		display: flex;
		align-items: center;
		gap: 10px;
		font-family: var(--font-heading);
		font-size: 1.125rem;
		font-weight: 600;
	}
	.chart-spot {
		font-family: var(--font-mono, monospace);
		font-size: 1rem;
		font-variant-numeric: tabular-nums;
	}
	.chart-sub {
		margin-top: 2px;
		font-size: 0.75rem;
		color: var(--muted-foreground);
	}
	.ranges {
		display: flex;
		gap: 4px;
	}
	.range {
		padding: 4px 10px;
		font-size: 0.75rem;
		border-radius: 9999px;
		color: var(--muted-foreground);
		transition: color 0.12s ease, background 0.12s ease;
	}
	.range:hover {
		color: var(--foreground);
	}
	.range[data-active='true'] {
		background: var(--primary);
		color: var(--primary-foreground);
		font-weight: 600;
	}
	.chart-body {
		flex: 1;
		min-height: 0;
	}
	.swap-card {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 18px 20px;
		min-height: 0;
		overflow-y: auto;
	}
	.swap-title {
		display: flex;
		align-items: center;
		gap: 8px;
		font-family: var(--font-heading);
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: 2px;
	}
	.leg {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: color-mix(in oklch, var(--card) 50%, transparent);
	}
	.leg-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		font-size: 0.75rem;
		color: var(--muted-foreground);
	}
	.leg-bal {
		font-variant-numeric: tabular-nums;
	}
	.flip {
		display: grid;
		place-items: center;
		width: 32px;
		height: 32px;
		margin: -16px auto;
		z-index: 1;
		border: 1px solid var(--border);
		border-radius: 9999px;
		background: var(--card);
		color: var(--muted-foreground);
	}
	.flip:hover {
		color: var(--primary);
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
	.quote {
		font-size: 0.8125rem;
		color: var(--muted-foreground);
		min-height: 2.4em;
	}
	.quote-sub {
		display: block;
		margin-top: 2px;
		font-size: 0.6875rem;
	}
	.slip {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		padding-top: 2px;
	}
	.slip-l {
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
	.slip-in {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 0.875rem;
		color: var(--muted-foreground);
	}
	.note {
		font-size: 0.6875rem;
		line-height: 1.4;
		color: var(--muted-foreground);
	}
</style>
