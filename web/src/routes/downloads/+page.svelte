<script lang="ts">
	import { onMount } from 'svelte';
	import { downloads } from '$lib/content/downloads';

	type Asset = { name: string; browser_download_url: string };
	let version = $state<string | null>(null);
	let assets = $state<Asset[]>([]);
	let loaded = $state(false);
	let userOs = $state<string>('');

	function assetFor(match: string[]): string | null {
		const hit = assets.find((a) =>
			match.some((m) => a.name.toLowerCase().endsWith(m.toLowerCase()))
		);
		return hit ? hit.browser_download_url : null;
	}

	onMount(async () => {
		const ua = navigator.userAgent;
		userOs = /Mac/i.test(ua)
			? 'macOS'
			: /Win/i.test(ua)
				? 'Windows'
				: /Linux/i.test(ua)
					? 'Linux'
					: '';
		try {
			const res = await fetch(downloads.latestApi, {
				headers: { Accept: 'application/vnd.github+json' }
			});
			if (res.ok) {
				const data = await res.json();
				version = data.tag_name ?? null;
				assets = Array.isArray(data.assets) ? data.assets : [];
			}
		} catch {
			// offline / rate-limited — fall back to the releases page links
		}
		loaded = true;
	});
</script>

<svelte:head><title>ozky — Download</title></svelte:head>

<section class="min-h-screen bg-grey px-8 pt-32 pb-20 text-ink">
	<div class="flex flex-wrap items-end justify-between gap-6">
		<h1
			class="font-display text-[clamp(3rem,9vw,7rem)] font-semibold leading-[0.85] tracking-[-0.04em]"
		>
			{downloads.heading}
		</h1>
		<a
			href={version ? downloads.latestReleaseUrl : downloads.releasesUrl}
			class="mono text-[11px] text-ink hover:opacity-60"
		>
			{loaded ? (version ?? 'latest release ↗') : 'checking latest…'}
		</a>
	</div>

	<p
		class="mt-6 max-w-[46ch] font-display text-[clamp(1.1rem,1.8vw,1.5rem)] font-medium leading-snug"
	>
		{downloads.blurb}
	</p>

	<!-- testnet banner -->
	<p class="mono mt-8 inline-block bg-ink px-4 py-2 text-[10px] text-gold">{downloads.notice}</p>

	<!-- OS cards -->
	<div class="mt-12 grid grid-cols-1 gap-px bg-ink md:grid-cols-3">
		{#each downloads.platforms as p (p.os)}
			{@const url = assetFor(p.match)}
			<div
				class="flex flex-col bg-grey p-8 {userOs === p.os ? 'outline outline-2 outline-ink' : ''}"
			>
				{#if userOs === p.os}<span class="mono mb-3 text-[10px] text-ink">Detected</span>{/if}
				<h2 class="font-display text-2xl font-medium">{p.os}</h2>
				<p class="mono mt-2 text-[11px] text-ink">{p.note}</p>
				<a
					href={url ?? downloads.latestReleaseUrl}
					class="mono mt-8 inline-flex w-fit items-center rounded-full border border-ink px-7 py-3 text-[11px] leading-none text-ink transition-colors duration-300 hover:bg-ink hover:text-grey"
				>
					{loaded && url ? `Download ↓` : 'Get it on GitHub ↗'}
				</a>
			</div>
		{/each}
	</div>

	<!-- requirements + source -->
	<div class="mt-16 grid grid-cols-1 gap-10 lg:grid-cols-[1fr_1fr]">
		<div>
			<h3 class="mono text-[11px] text-ink">Requirements</h3>
			<ul class="mono mt-4 space-y-2 text-[11px] leading-[1.7] text-ink">
				{#each downloads.requirements as r (r)}
					<li>— {r}</li>
				{/each}
			</ul>
		</div>
		<div>
			<h3 class="mono text-[11px] text-ink">Prefer to build it yourself?</h3>
			<p class="mono mt-4 max-w-[40ch] text-[11px] leading-[1.7] text-ink">
				ozky is open source. Clone the repo and build the Tauri app from source.
			</p>
			<a
				href={downloads.sourceUrl}
				class="mono mt-6 inline-flex w-fit items-center rounded-full border border-ink px-7 py-3 text-[11px] leading-none text-ink transition-colors duration-300 hover:bg-ink hover:text-grey"
			>
				View source ↗
			</a>
		</div>
	</div>
</section>
