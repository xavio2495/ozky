<script lang="ts">
	import * as Select from '$lib/components/ui/select';
	import { ASSETS } from '$lib/assets';

	let { value = $bindable('USDC') }: { value?: string } = $props();
</script>

<Select.Root type="single" bind:value>
	<Select.Trigger class="h-12 w-full font-medium">
		{value || 'Select asset'}
	</Select.Trigger>
	<Select.Content>
		{#each ASSETS as a (a.code)}
			<Select.Item value={a.code} label={a.code}>
				<span class="flex items-center gap-2">
					<span class="size-2 rounded-full" style="background:{a.accent}"></span>
					<span class="font-medium">{a.code}</span>
					<span class="text-xs text-muted-foreground">{a.name}</span>
				</span>
			</Select.Item>
		{/each}
	</Select.Content>
</Select.Root>
