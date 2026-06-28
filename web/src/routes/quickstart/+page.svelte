<script lang="ts">
	import { onMount } from 'svelte';
	import { gsap, ScrollTrigger, reducedMotion } from '$lib/scroll';
	import { quickstart } from '$lib/content/quickstart';
	import { social } from '$lib/content/site';

	const { steps } = quickstart;

	let section = $state<HTMLElement>();

	onMount(() => {
		if (!section) return;

		gsap.registerPlugin(ScrollTrigger);
		const root = section;
		const q = (s: string) => Array.from(root.querySelectorAll<HTMLElement>(s));
		const tweens: gsap.core.Tween[] = [];

		// on load: each block fills its colour in from top → bottom
		tweens.push(
			gsap.fromTo(
				q('[data-load]'),
				{ clipPath: 'inset(0 0 100% 0)' },
				{ clipPath: 'inset(0 0 0% 0)', duration: 1.2, ease: 'power2.inOut', stagger: 0.12 }
			)
		);
		// title reveals bottom → top behind its mask
		tweens.push(
			gsap.from(q('[data-title-line]'), {
				yPercent: 115,
				duration: 1.2,
				ease: 'power3.inOut',
				delay: 0.15
			})
		);
		// each step rises in once on screen
		q('[data-step]').forEach((el) => {
			tweens.push(
				gsap.from(el, {
					y: 40,
					autoAlpha: 0,
					duration: 1.2,
					ease: 'power2.inOut',
					scrollTrigger: { trigger: el, start: 'top 90%' }
				})
			);
		});

		ScrollTrigger.refresh();
		void reducedMotion();
		return () => tweens.forEach((t) => (t.scrollTrigger?.kill(), t.kill()));
	});
</script>

<svelte:head><title>ozky — Quickstart</title></svelte:head>

<section bind:this={section} data-nav class="flex w-full items-start">
	<!-- LEFT — title + video band (3/4). Pins to the top so the title stays locked
	     while the steps scroll; resumes once the steps column reaches its end. -->
	<div class="flex w-full flex-col lg:sticky lg:top-0 lg:h-fit lg:w-3/4 lg:self-start">
		<div data-load class="flex h-[75dvh] flex-col justify-center bg-gold px-8 pt-24 text-ink">
			<h1
				class="overflow-hidden font-display text-[clamp(3.5rem,11vw,10rem)] font-semibold leading-[0.8] tracking-[-0.04em]"
			>
				<span data-title-line class="block">{quickstart.heading}</span>
			</h1>
			<p
				class="mt-6 max-w-[34ch] font-display text-[clamp(1.05rem,1.6vw,1.5rem)] font-medium leading-snug"
			>
				{quickstart.blurb}
			</p>
		</div>

		<div data-load data-nav="light" class="h-[80dvh] w-full overflow-hidden">
			<img src="/img/banner_pop.png" alt="ozky on Stellar" class="h-full w-full object-cover" />
		</div>
	</div>

	<!-- RIGHT — index + steps (1/4); scrolls past the pinned left column -->
	<aside class="flex w-full flex-col lg:w-1/4">
		<!-- index — System-Status-style list of the steps -->
		<div data-load data-nav="light" class="flex flex-col bg-ink px-7 py-7 text-grey">
			<h2 class="font-display text-xl font-medium">Quickstart</h2>
			<ol class="mono mt-4 space-y-[5px] text-[10px]">
				{#each steps as step, i (step.title)}
					<li>
						<a
							href={`#step-${i + 1}`}
							class="flex items-center justify-between gap-2 hover:text-gold"
						>
							<span>{String(i + 1).padStart(2, '0')}. {step.title}</span>
							<span class="text-gold">&bull;</span>
						</a>
					</li>
				{/each}
			</ol>
		</div>

		<!-- steps -->
		{#each steps as step, i (step.title)}
			<div
				data-step
				id={`step-${i + 1}`}
				class="flex flex-col bg-grey px-7 py-8 text-ink {i > 0 ? '-mt-px border-t border-ink' : ''}"
			>
				<span class="font-display text-4xl font-semibold leading-none tracking-[-0.03em]">
					{String(i + 1).padStart(2, '0')}
				</span>
				<h3 class="mt-4 font-display text-lg font-medium leading-tight">{step.title}</h3>
				<p class="mono mt-3 text-[11px] leading-[1.8] text-ink">{step.body}</p>
				{#if step.href}
					<a
						href={step.href}
						class="mono mt-4 inline-flex w-fit items-center rounded-full border border-ink bg-ink px-5 py-2.5 text-[10px] leading-none text-gold transition-opacity duration-300 hover:opacity-80"
					>
						{step.hrefLabel ?? 'Open ↗'}
					</a>
				{/if}
			</div>
		{/each}

		<!-- next steps -->
		<div class="flex flex-col gap-3 bg-gold px-7 py-8 text-ink">
			<a
				href="/docs"
				class="mono inline-flex w-fit items-center rounded-full border border-ink bg-ink px-6 py-3 text-[11px] leading-none text-gold transition-opacity duration-300 hover:opacity-80"
			>
				Read the docs ↗
			</a>
			<a
				href={social.telegram}
				class="mono inline-flex w-fit items-center rounded-full border border-ink px-6 py-3 text-[11px] leading-none text-ink transition-colors duration-300 hover:bg-ink hover:text-gold"
			>
				Get help on Telegram ↗
			</a>
			<p class="mono mt-2 text-[10px] text-ink">{quickstart.footerNote}</p>
		</div>
	</aside>
</section>
