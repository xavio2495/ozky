<script lang="ts">
	// Error / 404 page — styled like the Loader: black screen, full-page rune-glitch.
	// Rendered full-screen (fixed, above header/footer) so the footer is hidden.
	// Main page links sit justified across the bottom.
	import { page } from '$app/state';
	import RuneGlitch from '$lib/components/home/RuneGlitch.svelte';
	import { nav } from '$lib/content/site';

	let status = $derived(page.status);
	let message = $derived(page.error?.message ?? 'Page not found');
</script>

<svelte:head><title>ozky — {status}</title></svelte:head>

<div class="fixed inset-0 z-[80] flex flex-col overflow-hidden bg-ink">
	<!-- full-screen rune field (coarse cells = fast first paint) -->
	<RuneGlitch fill cell={42} gap={16} runeSize={26} />

	<!-- centered status -->
	<div class="relative z-10 flex flex-1 flex-col items-center justify-center px-8 text-center">
		<p class="mono text-[12px] text-gold">error {status}</p>
		<h1
			class="font-display mt-3 text-[clamp(5rem,24vw,20rem)] leading-[0.85] font-semibold tracking-[-0.05em] text-gold"
		>
			{status}
		</h1>
		<p class="mono mt-4 text-[12px] text-grey">{message}</p>
	</div>

	<!-- main page links — justified across the bottom, no footer -->
	<nav class="relative z-10 flex flex-wrap items-center justify-between gap-x-6 gap-y-3 px-8 pb-8">
		{#each nav as item (item.label)}
			<a href={item.href} class="mono text-[11px] text-gold transition-colors hover:text-grey">
				{item.label}
			</a>
		{/each}
	</nav>
</div>
