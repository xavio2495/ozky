<script lang="ts">
	import { onMount } from 'svelte';
	import { gsap, ScrollTrigger, reducedMotion } from '$lib/scroll';
	import Button from '$lib/components/ui/Button.svelte';
	import RuneGlitch from '$lib/components/home/RuneGlitch.svelte';
	import Tetra from '$lib/components/graphics/Tetra.svelte';
	import Globe from '$lib/components/graphics/Globe.svelte';
	import Starburst from '$lib/components/graphics/Starburst.svelte';
	import Halftone from '$lib/components/graphics/Halftone.svelte';
	import { pitch } from '$lib/content/pitch';

	const icons = [Tetra, Globe, Starburst, Halftone];

	let root = $state<HTMLElement>();

	onMount(() => {
		if (!root || reducedMotion()) return;
		gsap.registerPlugin(ScrollTrigger);
		const host = root;
		const tweens: gsap.core.Tween[] = [];

		// Each slide replays the about-page reveal as it scrolls into view.
		host.querySelectorAll<HTMLElement>('[data-slide]').forEach((slide) => {
			const giant = slide.querySelectorAll<HTMLElement>('[data-giant]');
			const fill = slide.querySelectorAll<HTMLElement>('[data-fill]');
			const rise = slide.querySelectorAll<HTMLElement>('[data-rise]');
			const trigger = { trigger: slide, start: 'top 80%' };

			tweens.push(
				gsap.from(giant, {
					yPercent: 115,
					duration: 1.1,
					ease: 'power3.inOut',
					scrollTrigger: trigger
				})
			);
			tweens.push(
				gsap.fromTo(
					fill,
					{ clipPath: 'inset(0 0 100% 0)' },
					{
						clipPath: 'inset(0 0 0% 0)',
						duration: 1.1,
						ease: 'power2.inOut',
						scrollTrigger: trigger
					}
				)
			);
			tweens.push(
				gsap.from(rise, {
					y: 40,
					autoAlpha: 0,
					duration: 1,
					ease: 'power2.inOut',
					stagger: 0.08,
					scrollTrigger: trigger
				})
			);
		});

		ScrollTrigger.refresh();
		return () => tweens.forEach((t) => (t.scrollTrigger?.kill(), t.kill()));
	});
</script>

<div bind:this={root} data-nav class="relative bg-gold text-ink">
	<!-- slides — vertical single scroll. Sits BEHIND the rune band (below), so incoming
	     content is hidden until it scrolls up past the band. -->
	<div class="relative z-0">
		{#each pitch as s, i (s.n)}
			{@const Icon = icons[i % icons.length]}
			<section data-slide class="flex min-h-[82svh] flex-col px-7 pt-28 pb-[34svh]">
				{#if i === 0}
					<img
						data-rise
						src="/img/logo_with_icon_b.svg"
						alt="ozky"
						class="mb-8 block h-[clamp(3.2rem,11vw,4rem)] w-auto select-none"
					/>
				{/if}
				<p data-rise class="mono text-[11px] text-ink">{s.n} — {s.kicker}</p>
				<h2 class="mt-3 -mb-[0.18em] overflow-hidden pb-[0.18em]">
					<span
						data-giant
						class="font-display block text-[clamp(2.2rem,9vw,3.4rem)] leading-[1.02] font-semibold tracking-[-0.03em]"
					>
						{s.title}
					</span>
				</h2>

				<!-- content box -->
				<div data-fill class="mt-7 bg-ink p-7 text-grey">
					<Icon class="mb-5 h-10 w-10 text-gold" />
					<p class="text-[clamp(0.95rem,3.4vw,1.1rem)] leading-relaxed text-grey">{s.body}</p>

					{#if s.points}
						<ul class="mono mt-5 space-y-2 text-[11px] leading-relaxed text-grey">
							{#each s.points as p (p)}
								<li class="flex gap-3"><span class="text-gold">—</span><span>{p}</span></li>
							{/each}
						</ul>
					{/if}

					{#if s.cta}
						<div class="mt-6 flex flex-wrap gap-3">
							{#each s.cta as c (c.label)}
								<Button href={c.href} variant="solid-light">{c.label}</Button>
							{/each}
						</div>
					{/if}
				</div>
			</section>
		{/each}
	</div>

	<!-- rune band — opaque gold, pinned to the bottom of the viewport IN FRONT of the
	     slides so incoming content stays hidden behind it until it scrolls above. The
	     negative top margin overlaps the last slide (adds no page height); when the deck
	     ends the band unsticks and scrolls up, letting the footer come in. -->
	<div
		class="pointer-events-none sticky bottom-0 z-10 -mt-[32svh] h-[32svh] overflow-hidden bg-gold"
		aria-hidden="true"
	>
		<div class="flex h-full flex-col justify-end">
			<RuneGlitch fill base="var(--color-ink)" accent="var(--color-ink)" />
		</div>
	</div>
</div>
