<script lang="ts">
	import { onMount } from 'svelte';
	import RuneGlitch from './RuneGlitch.svelte';
	import { gsap, ScrollTrigger, reducedMotion } from '$lib/scroll';
	import { lander } from '$lib/content/home';

	const { status, feats } = lander;

	let section = $state<HTMLElement>();
	let rightCol = $state<HTMLElement>();
	let stickyTop = $state(0); // bottom-pin offset for the right column

	function measure() {
		if (!rightCol) return;
		stickyTop = window.innerHeight - rightCol.offsetHeight;
	}

	onMount(() => {
		measure();
		window.addEventListener('resize', measure);

		if (!section) {
			return () => window.removeEventListener('resize', measure);
		}

		// Reveals run regardless of the OS reduce-motion setting (explicitly requested);
		// only the continuous arrow loop is suppressed under reduced motion.
		const rm = reducedMotion();
		const DUR = 1.2; // each animation section lasts 1.2s

		// ScrollTrigger is registered in the layout, but children mount before the
		// parent — register here too (idempotent) so triggers created now are valid.
		gsap.registerPlugin(ScrollTrigger);
		const root = section;
		const q = (sel: string) => Array.from(root.querySelectorAll<HTMLElement>(sel));
		const tweens: gsap.core.Tween[] = [];

		// --- on load: each section's colour fills in from top → bottom ---
		// The page starts at its default grey (#BABABA); every block wipes its own
		// colour downward over that grey via a top-anchored clip-path.
		tweens.push(
			gsap.fromTo(
				q('[data-load]'),
				{ clipPath: 'inset(0 0 100% 0)' },
				{
					clipPath: 'inset(0 0 0% 0)',
					duration: 2,
					ease: 'power2.inOut',
					stagger: 0.12
				}
			)
		);
		// title lines reveal bottom → top behind their mask
		tweens.push(
			gsap.from(q('[data-title-line]'), {
				yPercent: 115,
				duration: 3,
				ease: 'power3.inOut',
				stagger: 0.14,
				delay: 0.15
			})
		);
		// explore arrow shuttles along the NE ↔ SW diagonal — begins once the explore
		// panel is 50% on screen (continuous; skipped under reduced motion)
		const arrow = q('[data-arrow]')[0];
		const explore = q('[data-explore]')[0];
		if (!rm && arrow && explore) {
			tweens.push(
				gsap.to(arrow, {
					x: -8,
					y: 8,
					repeat: -1,
					yoyo: true,
					duration: 1.5,
					ease: 'sine.inOut',
					scrollTrigger: { trigger: explore, start: 'top 50%' }
				})
			);
		}

		// --- on screen (not on load) ---
		// features and explore rise in only once scrolled into view
		q('[data-onscreen]').forEach((el) => {
			tweens.push(
				gsap.from(el, {
					y: 56,
					autoAlpha: 0,
					duration: DUR,
					ease: 'power2.inOut',
					scrollTrigger: { trigger: el, start: 'top 85%' }
				})
			);
		});
		// each feature border draws from NW → SE on scroll-in
		q('[data-feat-border]').forEach((el) => {
			tweens.push(
				gsap.fromTo(
					el,
					{ clipPath: 'inset(0 100% 100% 0)' },
					{
						clipPath: 'inset(0 0% 0% 0)',
						duration: 4,
						ease: 'power2.inOut',
						scrollTrigger: { trigger: el, start: 'top 85%' }
					}
				)
			);
		});

		// Recompute trigger positions after Lenis/layout/fonts settle.
		ScrollTrigger.refresh();

		return () => {
			window.removeEventListener('resize', measure);
			tweens.forEach((t) => {
				t.scrollTrigger?.kill();
				t.kill();
			});
		};
	});
</script>

<section bind:this={section} data-nav class="relative flex w-full items-start">
	<!-- LEFT COLUMN — 3/4 width: title · video · features -->
	<div class="flex w-3/4 flex-col">
		<!-- title — 75dvh, gold -->
		<div data-load class="flex h-[75dvh] flex-col justify-center bg-gold px-8 pt-24 text-ink">
			<div class="flex flex-wrap items-start gap-x-12 gap-y-4">
				<h1
					class="overflow-hidden font-display text-[clamp(4rem,12vw,11rem)] font-semibold leading-[0.8] tracking-[-0.04em]"
				>
					<span data-title-line class="block">{lander.titleTop}</span>
				</h1>
				<p
					class="mt-3 max-w-[22ch] font-display text-[clamp(1rem,1.6vw,1.4rem)] font-medium leading-tight"
				>
					{lander.subhead}
				</p>
			</div>
			<h1
				class="overflow-hidden font-display text-[clamp(4rem,12vw,11rem)] font-semibold leading-[0.8] tracking-[-0.045em]"
			>
				<span data-title-line class="block">{lander.titleBottom}</span>
			</h1>
		</div>

		<!-- video band — banner_pop (dark image → light nav tone) -->
		<div data-load data-nav="light" class="h-[80dvh] w-full overflow-hidden">
			<img src="/img/banner_pop.png" alt="ozky on Stellar" class="h-full w-full object-cover" />
		</div>

		<!-- features — three 3:2 cards, each with an NW→SE drawn border -->
		<div class="grid grid-cols-3">
			{#each feats as feat (feat.title)}
				<article data-onscreen class="relative aspect-[3/2] p-8">
					<div
						data-feat-border
						class="pointer-events-none absolute inset-0 border border-ink"
					></div>
					<p class="mono text-[10px] text-ink">{feat.tag}</p>
					<h3
						class="mt-4 font-display text-[clamp(1rem,1.4vw,1.35rem)] font-medium leading-tight text-ink"
					>
						{feat.title}
					</h3>
					<p class="mono absolute right-8 bottom-8 left-8 text-[10px] leading-[1.7] text-ink">
						{feat.body}
					</p>
				</article>
			{/each}
		</div>
	</div>

	<!-- RIGHT COLUMN — 1/4 width: tagline · system status · explore.
	     Bottom-pins (sticky) once its base reaches the viewport bottom. -->
	<aside
		bind:this={rightCol}
		class="sticky flex w-1/4 flex-col self-start"
		style:top="{stickyTop}px"
	>
		<!-- tagline — 50dvh, anchored to the bottom, grey -->
		<div data-load class="flex h-[50dvh] items-end bg-grey px-7 pb-8 text-ink">
			<p class="font-display text-[clamp(1.05rem,1.4vw,1.35rem)] font-medium leading-snug">
				{lander.tagline}
			</p>
		</div>

		<!-- system status — 50dvh, ink, small text + rune glitch -->
		<div data-load data-nav="light" class="flex h-[65dvh] flex-col bg-ink px-7 py-7 text-grey">
			<h3 class="font-display text-xl font-medium">System Status</h3>
			<ol class="mono mt-4 space-y-[3px] text-[9px] text-grey">
				{#each status as item, i (item)}
					<li class="flex items-center justify-between gap-2">
						<span>{String(i + 1).padStart(2, '0')}. {item}</span>
						<span class="text-gold">&bull;</span>
					</li>
				{/each}
			</ol>
			<div class="mt-auto pt-6">
				<RuneGlitch />
			</div>
		</div>

		<!-- explore — square, gold, NE↔SW arrow -->
		<a
			href={lander.exploreHref}
			data-onscreen
			data-explore
			class="flex aspect-square flex-col justify-between bg-gold p-7 text-ink"
		>
			<img
				data-arrow
				src="/img/arrow_l.png"
				alt=""
				aria-hidden="true"
				class="h-[5.2rem] w-[5.2rem] self-end object-contain"
			/>
			<span class="font-display text-[clamp(1.1rem,1.5vw,1.6rem)] font-medium leading-tight">
				{lander.exploreLabel}
			</span>
		</a>
	</aside>
</section>
