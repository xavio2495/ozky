<script lang="ts">
	import { onMount } from 'svelte';
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import * as Chart from '$lib/components/ui/chart';
	import * as Card from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { AreaChart } from 'layerchart';
	import { curveMonotoneX } from 'd3-shape';
	import { api, errMessage, type Spot, type PricePoint } from '$lib/api';
	import { ASSETS } from '$lib/assets';
	import { toast } from 'svelte-sonner';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ArrowDownRightIcon from '@lucide/svelte/icons/arrow-down-right';

	const ranges = [
		{ label: '24h', days: 1 },
		{ label: '7d', days: 7 },
		{ label: '30d', days: 30 },
		{ label: '90d', days: 90 }
	];

	let spots = $state<Spot[]>([]);
	let asset = $state('XLM');
	let days = $state(7);
	let history = $state<{ t: Date; usd: number }[]>([]);
	let loadingChart = $state(false);

	const spot = $derived(spots.find((s) => s.code === asset));
	const chartConfig: Chart.ChartConfig = { usd: { label: 'Price (USD)', color: 'var(--primary)' } };

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
			const pts: PricePoint[] = await api.priceHistory(asset, days);
			history = pts.map((p) => ({ t: new Date(p.t), usd: p.usd }));
		} catch (e) {
			history = [];
			toast.error('Could not load chart', { description: errMessage(e) });
		} finally {
			loadingChart = false;
		}
	}

	const fmtUsd = (n: number) =>
		n >= 1
			? n.toLocaleString('en-US', { style: 'currency', currency: 'USD' })
			: `$${n.toFixed(4)}`;

	onMount(loadSpots);
	// Reload the chart whenever the asset or range changes.
	$effect(() => {
		asset;
		days;
		loadHistory();
	});
</script>

<Workspace title="Markets" subtitle="Live USD prices for supported assets">
	{#snippet main()}
		<div class="flex flex-col gap-6">
			<div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
				{#each ASSETS as a (a.code)}
					{@const s = spots.find((x) => x.code === a.code)}
					<button
						class="ticker"
						data-active={asset === a.code}
						onclick={() => (asset = a.code)}
					>
						<div class="flex items-center justify-between">
							<span class="font-medium">{a.code}</span>
							<span class="size-2 rounded-full" style="background:{a.accent}"></span>
						</div>
						{#if s}
							<div class="mt-1 font-mono text-sm">{fmtUsd(s.usd)}</div>
							<div class="mt-0.5 flex items-center gap-1 text-xs" data-up={s.change_24h >= 0}>
								{#if s.change_24h >= 0}<ArrowUpRightIcon class="size-3" />{:else}<ArrowDownRightIcon
										class="size-3"
									/>{/if}
								{Math.abs(s.change_24h).toFixed(2)}%
							</div>
						{:else}
							<Skeleton class="mt-2 h-4 w-16" />
						{/if}
					</button>
				{/each}
			</div>

			<Card.Root>
				<Card.Header class="flex-row items-start justify-between gap-4">
					<div>
						<Card.Title class="flex items-center gap-2">
							{asset}
							{#if spot}
								<span class="font-mono text-base">{fmtUsd(spot.usd)}</span>
								<Badge variant={spot.change_24h >= 0 ? 'secondary' : 'destructive'} class="gap-1">
									{spot.change_24h >= 0 ? '+' : ''}{spot.change_24h.toFixed(2)}% 24h
								</Badge>
							{/if}
						</Card.Title>
						<Card.Description>USD price, last {days === 1 ? '24 hours' : `${days} days`}</Card.Description>
					</div>
					<div class="flex gap-1">
						{#each ranges as r (r.days)}
							<Button
								variant={days === r.days ? 'secondary' : 'ghost'}
								size="sm"
								class="h-7 px-2.5 text-xs"
								onclick={() => (days = r.days)}
							>
								{r.label}
							</Button>
						{/each}
					</div>
				</Card.Header>
				<Card.Content>
					{#if loadingChart && history.length === 0}
						<Skeleton class="h-[260px] w-full rounded-lg" />
					{:else if history.length === 0}
						<div class="grid h-[260px] place-items-center text-sm text-muted-foreground">
							No price data.
						</div>
					{:else}
						<Chart.Container config={chartConfig} class="h-[260px] w-full">
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
				</Card.Content>
			</Card.Root>
		</div>
	{/snippet}
</Workspace>

<style>
	.ticker {
		padding: 12px 14px;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: color-mix(in oklch, var(--card) 50%, transparent);
		text-align: left;
		transition: border-color 0.15s ease, transform 0.15s ease;
	}
	.ticker:hover {
		transform: translateY(-2px);
	}
	.ticker[data-active='true'] {
		border-color: var(--primary);
		background: color-mix(in oklch, var(--primary) 8%, transparent);
	}
	.ticker [data-up='true'] {
		color: oklch(0.72 0.15 150);
	}
	.ticker [data-up='false'] {
		color: var(--destructive);
	}
</style>
