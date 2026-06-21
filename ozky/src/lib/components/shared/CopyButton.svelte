<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import CheckIcon from '@lucide/svelte/icons/check';
	import { toast } from 'svelte-sonner';

	let {
		text,
		label = '',
		variant = 'outline',
		size = 'sm'
	}: {
		text: string;
		label?: string;
		variant?: 'outline' | 'ghost' | 'secondary';
		size?: 'sm' | 'icon';
	} = $props();

	let copied = $state(false);
	async function copy() {
		try {
			await navigator.clipboard.writeText(text);
			copied = true;
			toast.success('Copied to clipboard');
			setTimeout(() => (copied = false), 1500);
		} catch {
			toast.error('Could not copy');
		}
	}
</script>

<Button {variant} {size} onclick={copy} class={label ? 'gap-2' : ''}>
	{#if copied}
		<CheckIcon class="size-4 text-primary" />
	{:else}
		<CopyIcon class="size-4" />
	{/if}
	{#if label}{label}{/if}
</Button>
