<script lang="ts">
	import { onMount } from 'svelte';
	import Logo from './Logo.svelte';
	import { gsap, ScrollTrigger } from '$lib/scroll';
	import { nav, footer } from '$lib/content/site';

	let root = $state<HTMLElement>();

	onMount(() => {
		if (!root) return;
		gsap.registerPlugin(ScrollTrigger);
		const q = (s: string) => Array.from(root!.querySelectorAll<HTMLElement>(s));
		const tweens: gsap.core.Tween[] = [];

		// black box rises bottom → top
		tweens.push(
			gsap.from(q('[data-box]'), {
				y: 80,
				autoAlpha: 0,
				duration: 1.1,
				ease: 'power2.inOut',
				scrollTrigger: { trigger: root, start: 'top 85%' }
			})
		);
		// all footer text settles in top → bottom
		tweens.push(
			gsap.from(q('[data-ftext]'), {
				y: -28,
				autoAlpha: 0,
				duration: 1,
				ease: 'power2.inOut',
				stagger: 0.08,
				scrollTrigger: { trigger: root, start: 'top 80%' }
			})
		);
		ScrollTrigger.refresh();
		return () => tweens.forEach((t) => (t.scrollTrigger?.kill(), t.kill()));
	});
</script>

<footer bind:this={root} data-footer class="bg-gold px-8 pt-8 pb-8">
	<!-- black box — 80dvh, slightly rounded -->
	<div
		data-box
		class="flex min-h-[62dvh] flex-col rounded-[16px] bg-ink px-7 py-10 text-gold sm:px-10 sm:py-12 lg:h-[85dvh] lg:min-h-[85dvh]"
	>
		<!-- logo left · page nav right-aligned -->
		<div class="flex flex-wrap items-start gap-x-16 gap-y-10">
			<div data-ftext><Logo tone="gold" size={30} /></div>

			<nav class="ml-auto flex flex-wrap items-start justify-end gap-x-12 gap-y-6 text-right">
				{#each nav as item (item.label)}
					<div data-ftext class="font-display text-base leading-tight font-medium">
						<a href={item.href} class="transition-colors hover:text-grey">
							{item.label}{#if item.children}<sup class="ml-0.5 text-[0.5em]"
									>{item.children.length}</sup
								>{/if}
						</a>
						{#if item.children}
							<ul class="mt-1 space-y-0.5 text-sm">
								{#each item.children as c (c.label)}
									<li>
										<a href={c.href} class="transition-colors hover:text-grey">{c.label} ↲</a>
									</li>
								{/each}
							</ul>
						{/if}
					</div>
				{/each}
			</nav>
		</div>

		<!-- tagline + address + socials — justified across the box -->
		<div data-ftext class="mt-10 flex flex-row flex-wrap justify-between gap-4 lg:mt-14">
			<p class="mono max-w-[14ch] text-left text-[10px] leading-relaxed sm:text-[11px]">
				{#each footer.tagline as line (line)}{line}<br />{/each}
			</p>
			<p class="mono text-center text-[10px] leading-relaxed sm:text-[11px]">
				{#each footer.address as line (line)}{line}<br />{/each}
			</p>
			<ul class="mono space-y-1 text-right text-[10px] leading-relaxed sm:text-[11px]">
				{#each footer.links as l (l.label)}
					<li><a href={l.href} class="transition-colors hover:text-grey">{l.label} ↗</a></li>
				{/each}
			</ul>
		</div>

		<!-- giant wordmark (yellow) -->
		<img
			data-ftext
			src="/img/logo_with_icon.svg"
			alt="ozky"
			class="mt-auto block h-[22dvh] w-auto max-w-full self-center object-contain object-center pt-8 select-none lg:h-[35dvh]"
		/>
	</div>

	<!-- legal — below the box, centered -->
	<div
		data-ftext
		class="mono mt-6 flex flex-wrap justify-center gap-x-8 gap-y-2 text-[10px] text-ink"
	>
		{#each footer.legal as item (item.label)}
			{#if item.href}
				<a href={item.href} class="transition-opacity hover:opacity-60">{item.label}</a>
			{:else}
				<span>{item.label}</span>
			{/if}
		{/each}
	</div>
</footer>
