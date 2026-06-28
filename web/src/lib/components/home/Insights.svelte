<script lang="ts">
	import { onMount } from 'svelte';
	import { gsap, ScrollTrigger } from '$lib/scroll';
	import { integrates } from '$lib/content/home';

	const integrations = integrates.items;

	let section = $state<HTMLElement>();
	let leftCol = $state<HTMLElement>();
	let stickyTop = $state(0); // bottom-pin offset for the left column

	function measure() {
		if (!leftCol) return;
		stickyTop = window.innerHeight - leftCol.offsetHeight;
	}

	onMount(() => {
		measure();
		window.addEventListener('resize', measure);
		if (!section) return () => window.removeEventListener('resize', measure);

		gsap.registerPlugin(ScrollTrigger);
		const root = section;
		const q = (s: string) => Array.from(root.querySelectorAll<HTMLElement>(s));
		const one = (s: string) => root.querySelector<HTMLElement>(s);
		const tweens: gsap.core.Tween[] = [];

		// image fades in slowly
		tweens.push(
			gsap.from(q('[data-img]'), {
				autoAlpha: 0,
				duration: 1.8,
				ease: 'power2.inOut',
				scrollTrigger: { trigger: one('[data-img]'), start: 'top 92%' }
			})
		);
		// black text block fills in from top → bottom (same as the header sections)
		tweens.push(
			gsap.fromTo(
				q('[data-fill]'),
				{ clipPath: 'inset(0 0 100% 0)' },
				{
					clipPath: 'inset(0 0 0% 0)',
					duration: 1.2,
					ease: 'power2.inOut',
					scrollTrigger: { trigger: one('[data-fill]'), start: 'top 85%' }
				}
			)
		);
		// each cell rises in + its thin border draws NW → SE
		q('[data-cell]').forEach((el) => {
			tweens.push(
				gsap.from(el, {
					autoAlpha: 0,
					y: 40,
					duration: 1.2,
					ease: 'power2.inOut',
					scrollTrigger: { trigger: el, start: 'top 88%' }
				})
			);
		});
		q('[data-cell-border]').forEach((el) => {
			tweens.push(
				gsap.fromTo(
					el,
					{ clipPath: 'inset(0 100% 100% 0)' },
					{
						clipPath: 'inset(0 0% 0% 0)',
						duration: 1.2,
						ease: 'power2.inOut',
						scrollTrigger: { trigger: el, start: 'top 88%' }
					}
				)
			);
		});

		ScrollTrigger.refresh();

		return () => {
			window.removeEventListener('resize', measure);
			tweens.forEach((t) => (t.scrollTrigger?.kill(), t.kill()));
		};
	});
</script>

<section bind:this={section} id="insights" class="flex w-full items-start bg-grey text-ink">
	<!-- LEFT COLUMN — image + black text block. Bottom-pins once its base hits the viewport bottom. -->
	<div bind:this={leftCol} class="sticky flex w-1/2 flex-col self-start" style:top="{stickyTop}px">
		<div data-nav="light" class="h-[45dvh] w-full overflow-hidden">
			<img
				data-img
				src="/img/banner_sml.jpg"
				alt="ozky on Stellar"
				class="h-full w-full object-cover"
			/>
		</div>
		<div
			data-fill
			data-nav="light"
			class="flex h-[55dvh] flex-col justify-end bg-ink p-10 text-grey"
		>
			<h2
				class="font-display text-[clamp(2.2rem,4.4vw,4rem)] font-medium leading-[0.98] tracking-[-0.03em]"
			>
				{integrates.title}
			</h2>
			<p class="mono mt-5 max-w-[34ch] text-[11px] leading-[1.8] text-grey">
				{integrates.blurb}
			</p>
		</div>
	</div>

	<!-- RIGHT COLUMN — f1..fn, 2-up. Each cell draws only top+left borders so adjacent
	     edges never double up; the outer right/bottom edges are closed on the container. -->
	<div class="grid w-1/2 grid-cols-2 border-r border-b border-ink">
		{#each integrations as item (item.title)}
			<a
				href={item.href}
				data-cell
				class="group relative flex h-[45dvh] flex-col justify-end p-8 text-ink transition-colors duration-300 hover:bg-ink hover:text-grey"
			>
				<div
					data-cell-border
					class="pointer-events-none absolute inset-0 border-t border-l border-current"
				></div>
				<h3 class="font-display text-[clamp(1.4rem,2vw,2rem)] font-medium tracking-[-0.02em]">
					{item.title}
				</h3>
				<p class="mono mt-3 text-[10px] tracking-[0.08em]">{item.sub}</p>
			</a>
		{/each}
	</div>
</section>
