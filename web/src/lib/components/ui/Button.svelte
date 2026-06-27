<script lang="ts">
	import type { Snippet } from 'svelte';

	type Variant = 'outline' | 'solid-dark' | 'solid-light';

	let {
		href = undefined,
		variant = 'outline',
		children,
		...rest
	}: {
		href?: string;
		variant?: Variant;
		children: Snippet;
		[key: string]: unknown;
	} = $props();

	const base =
		'mono inline-flex items-center justify-center rounded-full border px-7 py-3 text-[11px] leading-none transition-colors duration-200';

	const variants: Record<Variant, string> = {
		// outline pill (EXPLORE on grey / READ MORE on dark)
		outline: 'border-current bg-transparent hover:bg-current/10',
		// solid dark pill (EXPLORE on orange card / SUBSCRIBE)
		'solid-dark': 'border-ink bg-ink text-orange hover:bg-coal',
		// solid light pill
		'solid-light': 'border-paper bg-paper text-ink hover:bg-white'
	};
</script>

{#if href}
	<a {href} class="{base} {variants[variant]}" {...rest}>{@render children()}</a>
{:else}
	<button class="{base} {variants[variant]}" {...rest}>{@render children()}</button>
{/if}
