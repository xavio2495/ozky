<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { gsap, ScrollTrigger } from '$lib/scroll';
	import { legalDocs } from '$lib/content/legal';

	let { children } = $props();
	let root = $state<HTMLElement>();

	$effect(() => {
		void page.url.pathname;
		if (!root) return;
		const els = Array.from(root.querySelectorAll<HTMLElement>('[data-rise]'));
		const tw = gsap.from(els, {
			y: 28,
			autoAlpha: 0,
			duration: 0.9,
			ease: 'power2.inOut',
			stagger: 0.06
		});
		return () => tw.kill();
	});

	onMount(() => {
		gsap.registerPlugin(ScrollTrigger);
		ScrollTrigger.refresh();
	});
</script>

<section data-nav="light" bind:this={root} class="min-h-screen bg-ink text-grey">
	<div class="grid grid-cols-1 lg:grid-cols-[1fr_3fr]">
		<!-- left rail — sibling-doc index -->
		<aside class="border-grey px-8 pt-32 pb-12 lg:sticky lg:top-0 lg:h-fit lg:self-start lg:pb-32">
			<nav class="mono space-y-2 text-[11px]">
				{#each legalDocs as d (d.slug)}
					{@const active = page.url.pathname === `/legal/${d.slug}`}
					<a
						href={`/legal/${d.slug}`}
						class="block border-l-2 py-0.5 pl-3 transition-colors hover:text-gold {active
							? 'border-gold text-gold'
							: 'border-transparent'}"
					>
						{d.label}
					</a>
				{/each}
			</nav>
		</aside>

		<!-- right — document body -->
		<div class="px-8 pt-32 pb-24 lg:pt-40">
			{@render children()}
		</div>
	</div>
</section>
