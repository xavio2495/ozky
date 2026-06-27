<script lang="ts">
	import './layout.css';
	import { onMount } from 'svelte';
	import Header from '$lib/components/Header.svelte';
	import MenuOverlay from '$lib/components/MenuOverlay.svelte';
	import Footer from '$lib/components/Footer.svelte';
	import { initScroll } from '$lib/scroll';
	import { meta } from '$lib/content/site';

	let { children } = $props();

	let menuOpen = $state(false);

	onMount(() => initScroll());
</script>

<svelte:head>
	<title>{meta.title}</title>
	<meta name="description" content={meta.description} />
</svelte:head>

<Header onMenu={() => (menuOpen = true)} />
<MenuOverlay open={menuOpen} onClose={() => (menuOpen = false)} />

<main>
	{@render children()}
</main>

<Footer />
