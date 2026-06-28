<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { gsap, ScrollTrigger } from '$lib/scroll';
	import { docsOrder, titleFor, neighbours } from '$lib/content/docs';

	let { children } = $props();

	let title = $derived(titleFor(page.url.pathname));
	let nb = $derived(neighbours(page.url.pathname));

	let content = $state<HTMLElement>();
	let rightCol = $state<HTMLElement>();
	let stickyTop = $state(0);

	function measure() {
		if (!rightCol) return;
		stickyTop = Math.min(0, window.innerHeight - rightCol.offsetHeight);
	}

	// Re-run reveal on client-side navigation between docs pages.
	$effect(() => {
		// re-read pathname so the effect re-runs on navigation
		void page.url.pathname;
		if (!content) return;
		const els = Array.from(content.querySelectorAll<HTMLElement>(':scope > *'));
		const tw = gsap.from(els, {
			y: 24,
			autoAlpha: 0,
			duration: 0.8,
			ease: 'power2.inOut',
			stagger: 0.04
		});
		measure();
		return () => tw.kill();
	});

	onMount(() => {
		measure();
		window.addEventListener('resize', measure);
		gsap.registerPlugin(ScrollTrigger);
		ScrollTrigger.refresh();
		return () => window.removeEventListener('resize', measure);
	});
</script>

<svelte:head><title>ozky — Docs · {title}</title></svelte:head>

<section data-nav="light" class="flex w-full items-start bg-ink text-grey">
	<!-- LEFT — documentation content (3/4), black page -->
	<div class="w-full px-8 pt-32 pb-20 lg:w-3/4">
		<!-- mobile index — title + sticky dropdown (desktop shows the right rail instead) -->
		<h1
			class="font-display mb-5 text-[clamp(1.8rem,7vw,2.4rem)] font-semibold leading-[0.95] tracking-[-0.03em] text-gold lg:hidden"
		>
			{title}
		</h1>
		<details class="sticky top-[68px] z-30 mb-10 border border-grey bg-ink lg:hidden">
			<summary
				class="mono flex cursor-pointer list-none items-center justify-between px-4 py-3 text-[11px] text-grey"
			>
				<span>Index</span>
				<span class="text-gold">▾</span>
			</summary>
			<ol
				class="mono max-h-[60vh] space-y-[6px] overflow-y-auto border-t border-grey px-4 py-4 text-[11px]"
			>
				{#each docsOrder as item, i (item.href)}
					{@const active = page.url.pathname === item.href}
					<li>
						<a
							href={item.href}
							class="flex items-center gap-2 py-0.5 transition-colors hover:text-gold {active
								? 'font-semibold text-gold'
								: ''} {item.level === 1 ? 'pl-4' : ''}"
						>
							<span>{String(i + 1).padStart(2, '0')}.</span>
							<span>{item.label}</span>
						</a>
					</li>
				{/each}
			</ol>
		</details>

		<div
			bind:this={content}
			class="prose prose-invert max-w-[70ch] prose-headings:font-display prose-headings:font-semibold prose-headings:tracking-[-0.02em] prose-a:text-gold prose-a:no-underline hover:prose-a:underline prose-strong:text-grey prose-code:text-gold prose-code:before:content-none prose-code:after:content-none [&_pre]:rounded-none [&_pre]:bg-gold [&_pre]:text-ink [&_pre_code]:text-ink"
		>
			{@render children()}
		</div>

		<!-- prev / next box nav -->
		<nav class="mt-20 grid grid-cols-1 gap-px border border-grey bg-grey sm:grid-cols-2">
			{#if nb.prev}
				<a
					href={nb.prev.href}
					class="group flex flex-col gap-2 bg-ink p-6 transition-colors hover:bg-gold hover:text-ink"
				>
					<span class="mono text-[10px] text-grey group-hover:text-ink">← Previous</span>
					<span class="font-display text-lg font-medium">{nb.prev.label}</span>
				</a>
			{:else}
				<div class="hidden bg-ink p-6 sm:block"></div>
			{/if}
			{#if nb.next}
				<a
					href={nb.next.href}
					class="group flex flex-col items-end gap-2 bg-ink p-6 text-right transition-colors hover:bg-gold hover:text-ink"
				>
					<span class="mono text-[10px] text-grey group-hover:text-ink">Next →</span>
					<span class="font-display text-lg font-medium">{nb.next.label}</span>
				</a>
			{/if}
		</nav>
	</div>

	<!-- RIGHT — sticky title (yellow) + numbered index (grey, black text) -->
	<aside
		bind:this={rightCol}
		class="sticky hidden w-1/4 flex-col self-start lg:flex"
		style:top="{stickyTop}px"
	>
		<!-- title — yellow, bottom-anchored -->
		<div data-title class="flex min-h-[34dvh] items-end bg-gold px-7 pt-32 pb-8 text-ink">
			<h1
				class="font-display text-[clamp(1.6rem,2.4vw,2.6rem)] font-semibold leading-[0.95] tracking-[-0.03em]"
			>
				{title}
			</h1>
		</div>

		<!-- index — grey with black text -->
		<nav class="flex min-h-[66dvh] flex-col bg-grey px-7 py-7 text-ink">
			<h2 class="font-display text-lg font-medium">Index</h2>
			<ol class="mono mt-4 space-y-[6px] text-[10px]">
				{#each docsOrder as item, i (item.href)}
					{@const active = page.url.pathname === item.href}
					<li>
						<a
							href={item.href}
							class="flex items-center gap-2 border-l-2 py-0.5 transition-colors hover:text-gold {active
								? 'border-ink pl-2 font-semibold'
								: 'border-transparent pl-2'} {item.level === 1 ? 'pl-5' : ''}"
						>
							<span>{String(i + 1).padStart(2, '0')}.</span>
							<span>{item.label}</span>
						</a>
					</li>
				{/each}
			</ol>
		</nav>
	</aside>
</section>
