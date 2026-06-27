<script lang="ts">
	// Two-segment composition donut (shielded vs public) built on shadcn Chart +
	// LayerChart PieChart — replaces the hand-rolled SVG ring. Center label overlays
	// the hole. Colors come from each datum's `color` field via the PieChart `c` accessor.
	import * as Chart from '$lib/components/ui/chart';
	import { PieChart } from 'layerchart';

	let {
		shielded = 0,
		pub = 0,
		label,
		sublabel,
		size = 132
	}: {
		shielded?: number;
		pub?: number;
		label?: string;
		sublabel?: string;
		size?: number;
	} = $props();

	// When there's nothing, show a single muted ring so the donut never collapses.
	const data = $derived(
		shielded + pub > 0
			? [
					{ name: 'shielded', value: shielded, color: 'var(--primary)' },
					{ name: 'public', value: pub, color: 'var(--muted)' }
				]
			: [{ name: 'empty', value: 1, color: 'var(--muted)' }]
	);

	const config: Chart.ChartConfig = {
		shielded: { label: 'Shielded', color: 'var(--primary)' },
		public: { label: 'Public', color: 'var(--muted)' }
	};
</script>

<div class="donut" style="width:{size}px; height:{size}px;">
	<Chart.Container {config} class="aspect-square size-full">
		<PieChart
			{data}
			key="name"
			value="value"
			c="color"
			innerRadius={0.68}
			padAngle={0.02}
			padding={0}
			props={{ pie: { motion: 'tween' } }}
		/>
	</Chart.Container>
	{#if label || sublabel}
		<div class="center">
			{#if label}<span class="lbl">{label}</span>{/if}
			{#if sublabel}<span class="sub">{sublabel}</span>{/if}
		</div>
	{/if}
</div>

<style>
	.donut {
		position: relative;
		display: grid;
		place-items: center;
	}
	.center {
		position: absolute;
		inset: 0;
		display: grid;
		place-items: center;
		text-align: center;
		pointer-events: none;
	}
	.lbl {
		font-family: var(--font-heading);
		font-size: 1.5rem;
		font-weight: 600;
		line-height: 1;
		font-variant-numeric: tabular-nums;
	}
	.sub {
		margin-top: 3px;
		font-size: 0.6875rem;
		color: var(--muted-foreground);
	}
</style>
