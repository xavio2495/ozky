<script lang="ts">
	import Tetra from '$lib/components/graphics/Tetra.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import { about } from '$lib/content/about';
	import { social } from '$lib/content/site';
</script>

<svelte:head><title>ozky — About</title></svelte:head>

<!-- hero -->
<section class="relative min-h-screen overflow-hidden bg-gold px-8 pt-32 pb-8 text-ink">
	<p
		class="mx-auto mt-[10vh] max-w-[20ch] text-center font-display text-[clamp(1.2rem,2vw,1.8rem)] font-medium leading-snug"
	>
		{about.hero}
	</p>

	<div class="absolute top-24 right-8 hidden w-[360px] bg-ink p-8 text-gold lg:block">
		<Tetra class="mb-6 h-12 w-12 text-gold" />
		<h2 class="font-display text-[clamp(1.4rem,1.8vw,1.7rem)] font-medium leading-tight">
			{about.mission.title}
		</h2>
		<p class="mono mt-5 text-[11px] leading-[1.8] text-gold">{about.mission.paras[0]}</p>
	</div>

	<h1
		class="absolute bottom-4 left-6 font-display text-[clamp(4rem,16vw,15rem)] font-semibold leading-[0.8] tracking-[-0.04em]"
	>
		{about.giant}
	</h1>
</section>

<!-- mission -->
<section class="bg-ink px-8 py-28 text-grey">
	<div class="grid grid-cols-1 gap-12 lg:grid-cols-[1fr_1.4fr]">
		<h2
			class="font-display text-[clamp(1.8rem,3vw,3rem)] font-medium leading-tight tracking-[-0.02em]"
		>
			{about.mission.title}
		</h2>
		<div class="mono space-y-6 text-[12px] leading-[1.9] text-grey">
			{#each about.mission.paras as para (para)}
				<p>{para}</p>
			{/each}
			<p class="text-gold">{about.noticeTestnet}</p>
		</div>
	</div>
</section>

<!-- how it works -->
<section class="bg-grey px-8 py-24 text-ink">
	<h2
		class="font-display max-w-[16ch] text-[clamp(1.8rem,3.4vw,3rem)] font-medium leading-[1.04] tracking-[-0.02em]"
	>
		{about.how.title}
	</h2>
	<div class="mt-12 grid grid-cols-1 gap-px bg-ink md:grid-cols-2">
		{#each about.how.points as p (p.k)}
			<div class="bg-grey p-8">
				<h3 class="font-display text-xl font-medium">{p.k}</h3>
				<p class="mono mt-3 max-w-[44ch] text-[11px] leading-[1.8] text-ink">{p.v}</p>
			</div>
		{/each}
	</div>
</section>

<!-- contact -->
<section class="bg-ink px-8 py-28 text-grey">
	<div class="grid grid-cols-1 gap-14 lg:grid-cols-[1fr_1.2fr]">
		<div>
			<h2
				class="font-display text-[clamp(2rem,4vw,3.4rem)] font-medium leading-[1.02] tracking-[-0.03em]"
			>
				{about.contact.title}
			</h2>
			<p class="mono mt-6 max-w-[36ch] text-[11px] leading-[1.8] text-grey">{about.contact.body}</p>
			<div class="mono mt-8 space-y-2 text-[11px]">
				<p>
					<a href={`mailto:${social.email}`} class="text-gold hover:opacity-70">{social.email}</a>
				</p>
				<p><a href={social.github} class="text-grey hover:text-gold">GitHub ↗</a></p>
				<p><a href={social.telegram} class="text-grey hover:text-gold">Telegram ↗</a></p>
			</div>
		</div>

		<form class="space-y-10" onsubmit={(e) => e.preventDefault()}>
			<div class="grid grid-cols-1 gap-10 sm:grid-cols-2">
				<label class="block">
					<span class="mono text-[10px] text-grey">{about.contact.fields.name}</span>
					<input
						type="text"
						class="mt-2 w-full border-0 border-b border-grey bg-transparent px-0 pb-2 font-display text-lg focus:border-gold focus:ring-0"
					/>
				</label>
				<label class="block">
					<span class="mono text-[10px] text-grey">{about.contact.fields.email}</span>
					<input
						type="email"
						class="mt-2 w-full border-0 border-b border-grey bg-transparent px-0 pb-2 font-display text-lg focus:border-gold focus:ring-0"
					/>
				</label>
			</div>
			<label class="block">
				<span class="mono text-[10px] text-grey">{about.contact.fields.message}</span>
				<textarea
					rows="3"
					class="mt-2 w-full resize-none border-0 border-b border-grey bg-transparent px-0 pb-2 font-display text-lg focus:border-gold focus:ring-0"
				></textarea>
			</label>
			<Button variant="solid-light">{about.contact.submit}</Button>
		</form>
	</div>
</section>
