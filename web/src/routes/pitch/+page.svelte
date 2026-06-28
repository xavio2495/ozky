<script lang="ts">
	import { onMount } from 'svelte';
	import PitchDeck from '$lib/components/pitch/PitchDeck.svelte';
	import PitchMobile from '$lib/components/pitch/PitchMobile.svelte';

	// Horizontal deck on desktop, vertical single-scroll on mobile. Start false to match
	// SSR (which can't know the viewport); flip after mount so the swap registers as a
	// reactive change. Below lg (1024px) → mobile.
	let isMobile = $state(false);

	onMount(() => {
		const mq = window.matchMedia('(max-width: 1023px)');
		isMobile = mq.matches;
		const onChange = (e: MediaQueryListEvent) => (isMobile = e.matches);
		mq.addEventListener('change', onChange);
		return () => mq.removeEventListener('change', onChange);
	});
</script>

<svelte:head><title>ozky — Pitch</title></svelte:head>

{#if isMobile}
	<PitchMobile />
{:else}
	<PitchDeck />
{/if}
