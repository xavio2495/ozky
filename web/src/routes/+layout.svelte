<script lang="ts">
	import './layout.css';
	import { onMount } from 'svelte';
	import Header from '$lib/components/Header.svelte';
	import MenuOverlay from '$lib/components/MenuOverlay.svelte';
	import Footer from '$lib/components/Footer.svelte';
	import Loader from '$lib/components/Loader.svelte';
	import { initScroll, releaseAnimations } from '$lib/scroll';
	import { meta } from '$lib/content/site';

	let { children } = $props();

	let menuOpen = $state(false);
	// Initial-load cover; removed once the loader finishes (full loads only).
	let loaded = $state(false);

	onMount(() => {
		const teardown = initScroll();
		// Safety: never leave GSAP playback paused if the loader fails to lift.
		const safety = setTimeout(releaseAnimations, 4000);
		return () => {
			clearTimeout(safety);
			teardown();
		};
	});
</script>

<svelte:head>
	<title>{meta.title}</title>
	<meta name="description" content={meta.description} />
</svelte:head>

{#if !loaded}
	<Loader onComplete={() => (loaded = true)} />
{/if}

<Header onMenu={() => (menuOpen = true)} />
<MenuOverlay open={menuOpen} onClose={() => (menuOpen = false)} />

<main>
	{@render children()}
</main>

<Footer />
