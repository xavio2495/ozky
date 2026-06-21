<script lang="ts">
	import { Tween } from 'svelte/motion';
	import { cubicOut } from 'svelte/easing';
	import { toNumber } from '$lib/format';
	import { assetByCode } from '$lib/assets';

	let { code, display }: { code: string; display: string } = $props();

	const meta = $derived(assetByCode(code));
	const tw = new Tween(0, { duration: 750, easing: cubicOut });
	$effect(() => {
		tw.set(toNumber(display));
	});
	const shown = $derived(
		tw.current.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
	);
</script>

<div class="stat" style="--accent:{meta?.accent ?? 'var(--primary)'}">
	<div class="bar"></div>
	<div class="flex items-baseline justify-between">
		<span class="font-medium">{code}</span>
		<span class="text-xs text-muted-foreground">{meta?.name ?? ''}</span>
	</div>
	<div class="mt-2 font-mono text-2xl font-semibold tracking-tight tabular-nums">{shown}</div>
</div>

<style>
	.stat {
		position: relative;
		padding: 18px 20px;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: color-mix(in oklch, var(--card) 60%, transparent);
		overflow: hidden;
		transition: transform 0.2s ease, border-color 0.2s ease;
	}
	.stat:hover {
		transform: translateY(-2px);
		border-color: color-mix(in oklch, var(--accent) 40%, var(--border));
	}
	.bar {
		position: absolute;
		top: 0;
		left: 0;
		width: 3px;
		height: 100%;
		background: var(--accent);
		opacity: 0.8;
	}
</style>
