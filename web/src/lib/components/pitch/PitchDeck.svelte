<script lang="ts">
	import { onMount } from 'svelte';
	import { gsap, reducedMotion } from '$lib/scroll';
	import Button from '$lib/components/ui/Button.svelte';
	import RuneGlitch from '$lib/components/home/RuneGlitch.svelte';
	import Tetra from '$lib/components/graphics/Tetra.svelte';
	import Globe from '$lib/components/graphics/Globe.svelte';
	import Starburst from '$lib/components/graphics/Starburst.svelte';
	import Halftone from '$lib/components/graphics/Halftone.svelte';
	import { pitch } from '$lib/content/pitch';

	// Cycle the line-art graphics through the slides (shown inside the dark box).
	const icons = [Tetra, Globe, Starburst, Halftone];

	let deck = $state<HTMLElement>();
	let active = $state(0);
	let raf = 0;
	let settle: ReturnType<typeof setTimeout>;
	let prev = -1;

	function els(i: number) {
		const slide = deck?.children[i] as HTMLElement | undefined;
		if (!slide) return null;
		return {
			giant: Array.from(slide.querySelectorAll<HTMLElement>('[data-giant]')),
			fill: Array.from(slide.querySelectorAll<HTMLElement>('[data-fill]')),
			rise: Array.from(slide.querySelectorAll<HTMLElement>('[data-rise]'))
		};
	}

	// Hold a slide hidden until it's revealed (prevents a flash before arrival).
	function hide(i: number) {
		if (reducedMotion()) return;
		const e = els(i);
		if (!e) return;
		gsap.set(e.giant, { yPercent: 115 });
		gsap.set(e.fill, { clipPath: 'inset(0 0 100% 0)' });
		gsap.set(e.rise, { y: 40, autoAlpha: 0 });
	}

	// About-page reveal, replayed every time a slide is landed on.
	function reveal(i: number) {
		if (reducedMotion()) return;
		const e = els(i);
		if (!e) return;
		gsap.fromTo(
			e.giant,
			{ yPercent: 115 },
			{ yPercent: 0, duration: 1.1, ease: 'power3.inOut', overwrite: true }
		);
		gsap.fromTo(
			e.fill,
			{ clipPath: 'inset(0 0 100% 0)' },
			{
				clipPath: 'inset(0 0 0% 0)',
				duration: 1.1,
				ease: 'power2.inOut',
				delay: 0.12,
				overwrite: true
			}
		);
		gsap.fromTo(
			e.rise,
			{ y: 40, autoAlpha: 0 },
			{
				y: 0,
				autoAlpha: 1,
				duration: 1,
				ease: 'power2.inOut',
				stagger: 0.08,
				delay: 0.1,
				overwrite: true
			}
		);
	}

	function go(i: number) {
		if (!deck) return;
		const n = Math.max(0, Math.min(pitch.length - 1, i));
		if (n !== active) hide(n); // hide the target before it scrolls in
		deck.scrollTo({ left: n * deck.clientWidth, behavior: reducedMotion() ? 'auto' : 'smooth' });
		active = n;
	}

	function onScroll() {
		if (!deck) return;
		cancelAnimationFrame(raf);
		raf = requestAnimationFrame(() => {
			if (!deck) return;
			active = Math.round(deck.scrollLeft / deck.clientWidth);
			if (active !== prev) {
				hide(active); // keep the incoming slide hidden mid-scroll
				prev = active;
			}
			window.dispatchEvent(new Event('scroll')); // header re-probes tone
			// reveal once the scroll has settled on a slide
			clearTimeout(settle);
			settle = setTimeout(() => reveal(active), 140);
		});
	}

	function onKey(e: KeyboardEvent) {
		if (e.key === 'ArrowRight') go(active + 1);
		else if (e.key === 'ArrowLeft') go(active - 1);
	}

	onMount(() => {
		// hide every slide, then reveal the first
		for (let i = 0; i < pitch.length; i++) hide(i);
		prev = 0;
		reveal(0);
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	});
</script>

<div class="relative h-[100svh] overflow-hidden">
	<!-- horizontal deck — every slide is gold -->
	<div
		bind:this={deck}
		onscroll={onScroll}
		class="no-scrollbar flex h-full snap-x snap-mandatory overflow-x-auto overflow-y-hidden"
	>
		{#each pitch as s, i (s.n)}
			{@const Icon = icons[i % icons.length]}
			<article
				data-nav="dark"
				class="relative h-full w-screen shrink-0 snap-start overflow-hidden bg-gold px-8 text-ink sm:px-14 lg:px-20"
			>
				<!-- decorative rune field, anchored to the bottom-left, visible slide only -->
				{#if active === i}
					<div
						class="pointer-events-none absolute bottom-14 left-3 flex h-[55%] w-[45%] flex-col justify-end overflow-hidden sm:left-6"
					>
						<RuneGlitch base="var(--color-ink)" accent="var(--color-ink)" />
					</div>
				{/if}

				<!-- TITLE — left, wide enough for the full line -->
				<div class="absolute top-28 left-8 max-w-[62%] sm:left-14 lg:top-32 lg:left-20">
					{#if i === 0}
						<img
							data-rise
							src="/img/logo_with_icon_b.svg"
							alt="ozky"
							class="mb-8 ml-auto block h-[clamp(4.2rem,5vh,4.6rem)] w-auto select-none"
						/>
					{/if}
					<p data-rise class="mono text-[11px] text-ink">{s.n} — {s.kicker}</p>
					<h2 class="mt-4 -mb-[0.18em] overflow-hidden pb-[0.18em]">
						<span
							data-giant
							class="font-display block text-[clamp(2rem,5.2vw,4.4rem)] leading-[1.02] font-semibold tracking-[-0.03em]"
						>
							{s.title}
						</span>
					</h2>
				</div>

				<!-- CONTENT — bottom-right, in a surrounding box -->
				<div
					data-fill
					class="absolute right-8 bottom-12 w-[min(90vw,440px)] bg-ink p-7 text-grey sm:right-14 lg:right-20"
				>
					<Icon class="mb-5 h-10 w-10 text-gold" />
					<p class="text-[clamp(0.95rem,1.7vh,1.15rem)] leading-relaxed text-grey">{s.body}</p>

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
			</article>
		{/each}
	</div>

	<!-- controls (ink on gold) -->
	<div class="pointer-events-none absolute inset-0 text-ink">
		<button
			onclick={() => go(active - 1)}
			disabled={active === 0}
			aria-label="Previous slide"
			class="mono pointer-events-auto absolute top-1/2 left-4 flex h-12 w-12 -translate-y-1/2 items-center justify-center rounded-full border border-current transition-opacity hover:opacity-60 disabled:opacity-20 sm:left-8"
		>
			←
		</button>
		<button
			onclick={() => go(active + 1)}
			disabled={active === pitch.length - 1}
			aria-label="Next slide"
			class="mono pointer-events-auto absolute top-1/2 right-4 flex h-12 w-12 -translate-y-1/2 items-center justify-center rounded-full border border-current transition-opacity hover:opacity-60 disabled:opacity-20 sm:right-8"
		>
			→
		</button>

		<nav
			class="pointer-events-auto absolute bottom-8 left-1/2 flex -translate-x-1/2 flex-wrap items-center justify-center gap-x-4 gap-y-2"
		>
			{#each pitch as s, i (s.n)}
				<button
					onclick={() => go(i)}
					aria-label={`Go to slide ${s.n}`}
					class="mono text-[11px] transition-opacity {active === i
						? 'opacity-100 underline underline-offset-4'
						: 'opacity-40 hover:opacity-80'}"
				>
					{s.n}
				</button>
			{/each}
		</nav>
	</div>
</div>

<style>
	.no-scrollbar {
		scrollbar-width: none;
	}
	.no-scrollbar::-webkit-scrollbar {
		display: none;
	}
</style>
