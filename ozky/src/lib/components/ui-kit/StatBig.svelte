<script lang="ts">
	// Headline KPI: a giant tabular number + icon + caption (design-language §7 StatBig).
	import type { Component } from 'svelte';

	let {
		value,
		caption,
		icon,
		unit
	}: {
		value: string | number;
		caption: string;
		icon: Component;
		unit?: string;
	} = $props();

	const Icon_ = $derived(icon);
</script>

<div class="stat">
	<span class="ico"><Icon_ class="size-[18px]" /></span>
	<div class="num">
		<span class="value">{value}</span>
		{#if unit}<span class="unit">{unit}</span>{/if}
	</div>
	<span class="caption">{caption}</span>
</div>

<style>
	.stat {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 20px 22px;
		border: 1px solid var(--border);
		border-radius: var(--radius-2xl);
		background: var(--card);
		backdrop-filter: blur(16px);
		-webkit-backdrop-filter: blur(16px);
	}
	.ico {
		display: grid;
		place-items: center;
		width: 34px;
		height: 34px;
		margin-bottom: 4px;
		border-radius: var(--radius-md);
		background: color-mix(in oklch, var(--primary) 14%, transparent);
		color: var(--primary);
	}
	.num {
		display: flex;
		align-items: baseline;
		gap: 6px;
	}
	.value {
		font-family: var(--font-heading);
		font-size: 2.25rem;
		font-weight: 600;
		line-height: 1;
		font-variant-numeric: tabular-nums;
	}
	.unit {
		font-size: 0.875rem;
		color: var(--muted-foreground);
	}
	.caption {
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
</style>
