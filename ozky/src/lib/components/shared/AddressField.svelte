<script lang="ts">
	import CopyButton from './CopyButton.svelte';
	import Qr from './Qr.svelte';

	let {
		label,
		value,
		hint,
		qr = false,
		loading = false
	}: {
		label: string;
		value: string;
		hint?: string;
		qr?: boolean;
		loading?: boolean;
	} = $props();
</script>

<div class="field">
	<div class="flex items-start justify-between gap-3">
		<div class="min-w-0">
			<p class="text-xs font-medium uppercase tracking-wider text-muted-foreground">{label}</p>
			<p class="mt-1.5 break-all font-mono text-sm leading-relaxed">
				{#if loading}
					<span class="text-muted-foreground">Loading…</span>
				{:else}
					{value}
				{/if}
			</p>
			{#if hint}<p class="mt-2 text-xs text-muted-foreground">{hint}</p>{/if}
		</div>
		{#if !loading && value}
			<CopyButton text={value} size="icon" />
		{/if}
	</div>

	{#if qr && value && !loading}
		<div class="mt-4 flex justify-center">
			<Qr data={value} />
		</div>
	{/if}
</div>

<style>
	.field {
		padding: 16px 18px;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: color-mix(in oklch, var(--card) 50%, transparent);
	}
</style>
