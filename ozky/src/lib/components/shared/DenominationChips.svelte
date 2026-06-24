<script lang="ts">
	// Standard-denomination quick-picks for the public edges (deposit/withdraw), where the amount IS
	// visible on-chain. Picking a common, round size blends your public amount into everyone else's —
	// the "sizing" half of the denomination/timing privacy policy (FEATURE_SET G7 / docs §7).
	import { Button } from '$lib/components/ui/button';
	import { STANDARD_DENOMINATIONS } from '$lib/settings.svelte';

	let { value = $bindable() }: { value: string } = $props();
</script>

<div class="flex flex-col gap-1.5">
	<div class="flex flex-wrap gap-1.5">
		{#each STANDARD_DENOMINATIONS as d (d)}
			<Button
				type="button"
				variant="outline"
				size="sm"
				class="h-7 px-2.5 font-mono text-xs"
				data-active={value === String(d)}
				onclick={() => (value = String(d))}
			>
				{d}
			</Button>
		{/each}
	</div>
	<p class="text-xs text-muted-foreground">Standard amounts blend in with other deposits and withdrawals.</p>
</div>
