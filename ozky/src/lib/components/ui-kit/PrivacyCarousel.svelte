<script lang="ts">
	// Privacy visualizations in a swipeable carousel: (1) shielded-vs-public donut,
	// (2) per-asset shielded/public stacked bars, (3) per-asset coverage list.
	// All shadcn Chart + LayerChart; colors from the gold ramp + muted.
	import * as Carousel from '$lib/components/ui/carousel';
	import type { CarouselAPI } from '$lib/components/ui/carousel/context';
	import * as Chart from '$lib/components/ui/chart';
	import CompositionDonut from './CompositionDonut.svelte';
	import { BarChart } from 'layerchart';

	type Asset = { code: string; shielded: number; pub: number; usd: number; coverage: number };
	let {
		shieldedUsd = 0,
		publicUsd = 0,
		coverageOverall = 0,
		assets = []
	}: {
		shieldedUsd?: number;
		publicUsd?: number;
		coverageOverall?: number;
		assets?: Asset[];
	} = $props();

	// Stacked bars need USD-valued shielded/public per asset (skip price-less assets).
	const barData = $derived(
		assets
			.filter((a) => a.usd > 0)
			.map((a) => ({
				asset: a.code,
				shielded: a.coverage * a.usd,
				public: (1 - a.coverage) * a.usd
			}))
	);
	const barConfig: Chart.ChartConfig = {
		shielded: { label: 'Shielded', color: 'var(--primary)' },
		public: { label: 'Public', color: 'var(--muted)' }
	};

	// Auto-advance every 5s; no manual arrows.
	let api = $state<CarouselAPI>();
	$effect(() => {
		if (!api) return;
		const id = setInterval(() => api?.scrollNext(), 5000);
		return () => clearInterval(id);
	});
</script>

<Carousel.Root class="w-full" opts={{ align: 'start', loop: true }} setApi={(a) => (api = a)}>
	<Carousel.Content>
		<!-- 1 · Composition donut -->
		<Carousel.Item>
			<div class="slide">
				<CompositionDonut
					shielded={shieldedUsd}
					pub={publicUsd}
					label="{Math.round(coverageOverall * 100)}%"
					size={140}
				/>
				<div class="legend">
					<span class="leg"><span class="sw gold"></span>Shielded</span>
					<span class="leg"><span class="sw muted"></span>Public</span>
				</div>
			</div>
		</Carousel.Item>

		<!-- 2 · Per-asset shielded/public stacked bars -->
		<Carousel.Item>
			<div class="slide">
				<span class="slide-title">By asset (USD)</span>
				{#if barData.length === 0}
					<p class="muted">No priced assets to chart.</p>
				{:else}
					<Chart.Container config={barConfig} class="aspect-auto h-[140px] w-[300px]">
						<BarChart
							data={barData}
							x="asset"
							seriesLayout="stack"
							series={[
								{ key: 'shielded', label: 'Shielded', value: 'shielded', color: 'var(--primary)' },
								{ key: 'public', label: 'Public', value: 'public', color: 'var(--muted)' }
							]}
							padding={{ top: 8, bottom: 20, left: 2, right: 2 }}
							props={{ bars: { radius: 6, strokeWidth: 0 } }}
						>
							{#snippet tooltip()}
								<Chart.Tooltip />
							{/snippet}
						</BarChart>
					</Chart.Container>
				{/if}
			</div>
		</Carousel.Item>

		<!-- 3 · Per-asset coverage list -->
		<Carousel.Item>
			<div class="slide">
				<span class="slide-title">Coverage by asset</span>
				{#if assets.length === 0}
					<p class="muted">No funds yet.</p>
				{:else}
					<div class="cov-list">
						{#each assets as a (a.code)}
							<div class="cov-row">
								<span class="cov-code">{a.code}</span>
								<div class="cov-bar">
									<div class="cov-fill" style="width:{Math.round(a.coverage * 100)}%"></div>
								</div>
								<span class="cov-pct">{Math.round(a.coverage * 100)}%</span>
							</div>
						{/each}
					</div>
				{/if}
			</div>
		</Carousel.Item>
	</Carousel.Content>
</Carousel.Root>

<style>
	.slide {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 10px;
		height: 100%;
		min-height: 150px;
		padding: 4px;
	}
	.slide-title {
		align-self: flex-start;
		font-size: 0.75rem;
		color: var(--muted-foreground);
	}
	.muted {
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
	.legend {
		display: flex;
		gap: 16px;
	}
	.leg {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		font-size: 0.75rem;
		color: var(--muted-foreground);
	}
	.sw {
		width: 11px;
		height: 11px;
		border-radius: 3px;
	}
	.sw.gold {
		background: var(--primary);
	}
	.sw.muted {
		background: var(--muted);
	}
	.cov-list {
		display: flex;
		flex-direction: column;
		gap: 10px;
		width: 100%;
	}
	.cov-row {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.cov-code {
		width: 44px;
		flex-shrink: 0;
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
	.cov-bar {
		position: relative;
		flex: 1;
		height: 10px;
		border-radius: 9999px;
		overflow: hidden;
		background: var(--muted);
	}
	.cov-fill {
		position: absolute;
		inset: 0 auto 0 0;
		border-radius: 9999px;
		background: var(--primary);
		transition: width 0.5s cubic-bezier(0.22, 1, 0.36, 1);
	}
	.cov-pct {
		width: 34px;
		flex-shrink: 0;
		text-align: right;
		font-size: 0.6875rem;
		font-variant-numeric: tabular-nums;
		color: var(--muted-foreground);
	}
</style>
