<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';

	let {
		value = $bindable(''),
		code = '',
		decimals = 7,
		max,
		placeholder = '0.00'
	}: {
		value?: string;
		code?: string;
		decimals?: number;
		/** spendable max in base units, enables a Max button. */
		max?: number;
		placeholder?: string;
	} = $props();

	function setMax() {
		if (max == null) return;
		const whole = Math.floor(max / 10 ** decimals);
		const frac = max % 10 ** decimals;
		value = frac ? `${whole}.${String(frac).padStart(decimals, '0').replace(/0+$/, '')}` : String(whole);
	}
</script>

<div class="relative">
	<Input
		bind:value
		type="text"
		inputmode="decimal"
		{placeholder}
		class="h-12 pr-24 font-mono text-lg"
	/>
	<div class="absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-1.5">
		{#if max != null}
			<Button variant="ghost" size="sm" class="h-7 px-2 text-xs" onclick={setMax}>Max</Button>
		{/if}
		{#if code}<span class="text-sm font-medium text-muted-foreground">{code}</span>{/if}
	</div>
</div>
