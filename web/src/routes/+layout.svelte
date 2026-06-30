<script lang="ts">
	import './layout.css';
	import { onMount } from 'svelte';
	import { afterNavigate } from '$app/navigation';
	import Header from '$lib/components/Header.svelte';
	import MenuOverlay from '$lib/components/MenuOverlay.svelte';
	import Footer from '$lib/components/Footer.svelte';
	import Loader from '$lib/components/Loader.svelte';
	import { initScroll, releaseAnimations, getLenis } from '$lib/scroll';
	import { meta } from '$lib/content/site';

	let { children } = $props();

	let menuOpen = $state(false);
	// Initial-load cover; removed once the loader finishes (full loads only).
	let loaded = $state(false);

	// Lenis tracks its own virtual scroll target, so SvelteKit's window.scrollTo(0,0) on
	// navigation gets overridden and the new page keeps the previous scroll position. Sync
	// Lenis after each navigation: jump to the top for new pages, or to the position the
	// browser restored on back/forward (popstate). Hash anchors are left to the browser.
	afterNavigate((nav) => {
		if (nav.to?.url.hash) return;
		const lenis = getLenis();
		const top = nav.type === 'popstate' ? window.scrollY : 0;
		if (lenis) lenis.scrollTo(top, { immediate: true });
		else if (nav.type !== 'popstate') window.scrollTo(0, 0);
	});

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

	<!-- Open Graph / social preview -->
	<meta property="og:type" content="website" />
	<meta property="og:title" content={meta.title} />
	<meta property="og:description" content={meta.description} />
	<meta property="og:url" content={meta.url} />
	<meta property="og:image" content={meta.image} />

	<!-- Twitter / X -->
	<meta name="twitter:card" content="summary_large_image" />
	<meta name="twitter:title" content={meta.title} />
	<meta name="twitter:description" content={meta.description} />
	<meta name="twitter:image" content={meta.image} />
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
