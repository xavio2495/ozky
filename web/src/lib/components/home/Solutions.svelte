<script lang="ts">
	import { onMount } from 'svelte';
	import Tetra from '../graphics/Tetra.svelte';
	import Globe from '../graphics/Globe.svelte';
	import Halftone from '../graphics/Halftone.svelte';
	import Starburst from '../graphics/Starburst.svelte';
	import { gsap, ScrollTrigger } from '$lib/scroll';
	import { solutions } from '$lib/content/home';

	const graphics = { tetra: Tetra, globe: Globe, halftone: Halftone, starburst: Starburst };
	const cards = solutions.cards;

	let section = $state<HTMLElement>();
	let track = $state<HTMLElement>();

	// Smooth, ease-in-out scroll on arrow click (no ScrollToPlugin dependency).
	function scrollByCard(dir: 1 | -1) {
		if (!track) return;
		const card = track.querySelector('article');
		const w = card ? card.getBoundingClientRect().width : track.clientWidth / 3;
		const target = Math.max(
			0,
			Math.min(track.scrollWidth - track.clientWidth, track.scrollLeft + dir * w)
		);
		const proxy = { x: track.scrollLeft };
		gsap.to(proxy, {
			x: target,
			duration: 0.8,
			ease: 'power2.inOut',
			onUpdate: () => track && (track.scrollLeft = proxy.x)
		});
	}

	onMount(() => {
		if (!section) return;
		gsap.registerPlugin(ScrollTrigger);
		const root = section;
		const q = (s: string) => Array.from(root.querySelectorAll<HTMLElement>(s));
		const tweens: gsap.core.Tween[] = [];

		// title reveals left → right as a block on scroll
		tweens.push(
			gsap.fromTo(
				q('[data-title]'),
				{ clipPath: 'inset(0 100% 0 0)' },
				{
					clipPath: 'inset(0 0% 0 0)',
					duration: 1.2,
					ease: 'power2.inOut',
					scrollTrigger: { trigger: root, start: 'top 80%' }
				}
			)
		);
		// arrow buttons slide in from outside the right edge on scroll
		tweens.push(
			gsap.from(q('[data-arrows]'), {
				xPercent: 180,
				autoAlpha: 0,
				duration: 1.2,
				ease: 'power2.inOut',
				scrollTrigger: { trigger: root, start: 'top 80%' }
			})
		);
		// each card's vector zooms in once on screen
		q('[data-vector]').forEach((el) => {
			tweens.push(
				gsap.from(el, {
					scale: 0.55,
					autoAlpha: 0,
					duration: 1.2,
					ease: 'power2.inOut',
					transformOrigin: 'center',
					scrollTrigger: { trigger: el, start: 'top 88%' }
				})
			);
		});

		ScrollTrigger.refresh();

		return () => tweens.forEach((t) => (t.scrollTrigger?.kill(), t.kill()));
	});
</script>

<section bind:this={section} id="solutions" class="bg-grey pt-16 text-ink">
	<!-- heading + arrows -->
	<div class="flex items-end justify-between gap-6 px-8 pb-10">
		<h2
			data-title
			class="font-display max-w-[14ch] text-[clamp(1.4rem,2.8vw,2.3rem)] leading-[1.05] font-normal tracking-[-0.02em]"
		>
			{solutions.heading}
		</h2>
		<div data-arrows class="flex shrink-0 gap-2.5">
			<button
				onclick={() => scrollByCard(-1)}
				aria-label="Previous"
				class="grid h-[2.1rem] w-[2.1rem] place-items-center rounded-full bg-ink text-grey transition-colors hover:bg-grey hover:text-ink"
			>
				<svg viewBox="0 0 24 24" class="h-3 w-3" fill="none"
					><path d="M15 5 L8 12 L15 19" stroke="currentColor" stroke-width="2.4" /></svg
				>
			</button>
			<button
				onclick={() => scrollByCard(1)}
				aria-label="Next"
				class="grid h-[2.1rem] w-[2.1rem] place-items-center rounded-full bg-ink text-grey transition-colors hover:bg-grey hover:text-ink"
			>
				<svg viewBox="0 0 24 24" class="h-3 w-3" fill="none"
					><path d="M9 5 L16 12 L9 19" stroke="currentColor" stroke-width="2.4" /></svg
				>
			</button>
		</div>
	</div>

	<!-- card track -->
	<div
		bind:this={track}
		class="flex snap-x snap-mandatory overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
	>
		{#each cards as card, i (card.title)}
			{@const G = graphics[card.graphic]}
			<article
				class="group flex min-h-[78vh] w-[88vw] shrink-0 snap-start flex-col border border-ink bg-grey p-9 transition-colors duration-300 hover:bg-gold sm:w-[60vw] lg:w-[calc(100%/3)] {i >
				0
					? '-ml-px'
					: ''}"
			>
				<div class="grid flex-1 place-items-center">
					<div data-vector>
						<G class="h-56 w-56 text-ink" />
					</div>
				</div>
				<h3 class="font-display text-[clamp(1.4rem,2vw,1.9rem)] font-medium tracking-[-0.02em]">
					{card.title}
				</h3>
				<p class="mono mt-4 max-w-[42ch] text-[11px] leading-[1.7] text-ink">{card.body}</p>
				<div class="mt-7">
					<a
						href={card.href}
						class="mono inline-flex items-center rounded-full border border-ink px-7 py-3 text-[11px] leading-none text-ink transition-colors duration-300 group-hover:bg-ink group-hover:text-grey"
					>
						Explore ↗
					</a>
				</div>
			</article>
		{/each}
	</div>
</section>
