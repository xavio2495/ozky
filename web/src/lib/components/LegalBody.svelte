<script lang="ts">
	import type { LegalDoc } from '$lib/content/legal';
	let { doc }: { doc: LegalDoc } = $props();
</script>

<svelte:head><title>ozky — {doc.label}</title></svelte:head>

<header class="max-w-[60ch]">
	<p data-rise class="mono text-[10px] text-gold">Updated {doc.updated}</p>
	<h1
		data-rise
		class="mt-4 font-display text-[clamp(3rem,9vw,7rem)] font-semibold leading-[0.85] tracking-[-0.04em] text-grey"
	>
		{doc.title}
	</h1>
	<p data-rise class="mt-8 max-w-[52ch] text-[clamp(1rem,1.4vw,1.2rem)] leading-relaxed text-grey">
		{doc.intro}
	</p>
</header>

<div class="mt-16 max-w-[64ch] space-y-12">
	{#each doc.sections as s (s.h)}
		<section data-rise>
			<h2 class="font-display text-[clamp(1.2rem,2vw,1.6rem)] font-medium tracking-[-0.02em]">
				{s.h}
			</h2>
			<div class="mt-4 space-y-4 text-[13px] leading-[1.85] text-grey">
				{#each s.p as para (para)}
					<p>{para}</p>
				{/each}
			</div>
		</section>
	{/each}
</div>
