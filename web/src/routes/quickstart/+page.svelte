<script lang="ts">
	import { quickstart } from '$lib/content/quickstart';
	import { social } from '$lib/content/site';
</script>

<svelte:head><title>ozky — Quickstart</title></svelte:head>

<section class="min-h-screen bg-gold px-8 pt-32 pb-20 text-ink">
	<h1
		class="font-display text-[clamp(3rem,9vw,7rem)] font-semibold leading-[0.85] tracking-[-0.04em]"
	>
		{quickstart.heading}
	</h1>
	<p
		class="mt-6 max-w-[44ch] font-display text-[clamp(1.1rem,1.8vw,1.5rem)] font-medium leading-snug"
	>
		{quickstart.blurb}
	</p>

	<!-- steps -->
	<ol class="mt-14 grid grid-cols-1 gap-px bg-ink md:grid-cols-2">
		{#each quickstart.steps as step, i (step.title)}
			<li class="flex flex-col bg-gold p-8">
				<span class="font-display text-5xl font-semibold leading-none tracking-[-0.03em]">
					{String(i + 1).padStart(2, '0')}
				</span>
				<h2 class="mt-6 font-display text-2xl font-medium leading-tight">{step.title}</h2>
				<p class="mono mt-3 max-w-[42ch] text-[11px] leading-[1.8] text-ink">{step.body}</p>
			</li>
		{/each}
	</ol>

	<!-- next steps -->
	<div class="mt-14 flex flex-wrap items-center gap-4">
		<a
			href="/docs"
			class="mono inline-flex items-center rounded-full border border-ink bg-ink px-7 py-3 text-[11px] leading-none text-gold transition-opacity duration-300 hover:opacity-80"
		>
			Read the docs ↗
		</a>
		<a
			href={social.telegram}
			class="mono inline-flex items-center rounded-full border border-ink px-7 py-3 text-[11px] leading-none text-ink transition-colors duration-300 hover:bg-ink hover:text-gold"
		>
			Get help on Telegram ↗
		</a>
	</div>

	<p class="mono mt-10 text-[10px] text-ink">{quickstart.footerNote}</p>
</section>
