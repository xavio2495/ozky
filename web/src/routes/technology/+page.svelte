<script lang="ts">
	import Texture from '$lib/components/Texture.svelte';
	import { technology } from '$lib/content/technology';
</script>

<svelte:head><title>ozky — Technology</title></svelte:head>

<section class="relative min-h-screen overflow-hidden bg-ink text-grey">
	<div class="grid min-h-screen grid-cols-1 lg:grid-cols-[1fr_360px]">
		<!-- left: tech copy + image + vertical wordmark -->
		<div class="relative px-8 pt-32 pb-16">
			<!-- image tile -->
			<div class="mt-10 w-full max-w-[340px] lg:absolute lg:top-[30%] lg:left-0 lg:mt-0">
				<Texture class="h-[200px] w-full" seed={21} />
			</div>

			<!-- the tech copy -->
			<div class="mt-12 max-w-[34ch] lg:absolute lg:top-[36%] lg:left-[36%] lg:mt-0">
				<h2 class="font-display text-2xl font-medium">{technology.title}</h2>
				<p class="mono mt-4 text-[11px] leading-[1.8] text-grey">
					{technology.body}
				</p>
			</div>

			<!-- giant vertical wordmark -->
			<span
				class="pointer-events-none absolute top-0 right-2 hidden font-display text-[12rem] font-medium leading-none tracking-[-0.03em] text-grey lg:block"
				style="writing-mode: vertical-rl;"
			>
				{technology.giant}
			</span>
		</div>

		<!-- right rail -->
		<aside class="flex flex-col">
			<Texture class="h-[55vh] w-full" seed={31} />
			<a
				href={technology.exploreHref}
				class="group flex flex-1 flex-col justify-between bg-gold p-7 text-ink"
			>
				<svg
					viewBox="0 0 100 100"
					class="h-20 w-20 self-end transition-transform duration-300 group-hover:translate-x-1 group-hover:-translate-y-1"
					fill="none"
					aria-hidden="true"
				>
					<path d="M22 78 L78 22 M40 22 L78 22 L78 60" stroke="currentColor" stroke-width="12" />
				</svg>
				<span class="font-display text-[clamp(1.3rem,1.8vw,1.8rem)] font-medium"
					>{technology.exploreLabel}</span
				>
			</a>
		</aside>
	</div>
</section>
