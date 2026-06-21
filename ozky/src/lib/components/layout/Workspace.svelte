<script lang="ts">
	// The right two columns of the 3-column shell: a scrollable main area with a page head,
	// plus an optional contextual aside (col 3) where future per-view panels slot in.
	import type { Snippet } from 'svelte';
	import { fly } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';

	let {
		title,
		subtitle,
		main,
		aside
	}: {
		title: string;
		subtitle?: string;
		main: Snippet;
		aside?: Snippet;
	} = $props();
</script>

<div class="workspace" class:has-aside={!!aside}>
	<section class="col-main">
		<header class="page-head" in:fly={{ y: 12, duration: 320, easing: cubicOut }}>
			<h1 class="font-heading text-2xl font-semibold tracking-tight">{title}</h1>
			{#if subtitle}<p class="mt-1 text-sm text-muted-foreground">{subtitle}</p>{/if}
		</header>
		<div in:fly={{ y: 16, duration: 380, delay: 60, easing: cubicOut }}>
			{@render main()}
		</div>
	</section>

	{#if aside}
		<aside class="col-aside" in:fly={{ x: 20, duration: 400, delay: 120, easing: cubicOut }}>
			{@render aside()}
		</aside>
	{/if}
</div>

<style>
	.workspace {
		display: grid;
		grid-template-columns: 1fr;
		gap: 28px;
		height: 100%;
		overflow-y: auto;
		padding: 32px 36px 40px;
	}
	.has-aside {
		grid-template-columns: minmax(0, 1fr) 320px;
	}
	@media (max-width: 1080px) {
		.has-aside {
			grid-template-columns: minmax(0, 1fr);
		}
		.col-aside {
			display: none;
		}
	}
	.page-head {
		margin-bottom: 24px;
	}
	.col-main {
		min-width: 0;
	}
</style>
