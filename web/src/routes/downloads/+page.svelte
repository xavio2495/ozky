<script lang="ts">
	import { onMount } from 'svelte';
	import Tetra from '$lib/components/graphics/Tetra.svelte';
	import Globe from '$lib/components/graphics/Globe.svelte';
	import Halftone from '$lib/components/graphics/Halftone.svelte';
	import { gsap, ScrollTrigger } from '$lib/scroll';
	import { downloads } from '$lib/content/downloads';

	type Asset = { name: string; browser_download_url: string; digest?: string | null };

	const graphics = { macOS: Tetra, Windows: Globe, Linux: Halftone };

	let version = $state<string | null>(null);
	let assets = $state<Asset[]>([]);
	let loaded = $state(false);
	let userOs = $state<'macOS' | 'Windows' | 'Linux' | ''>('');
	// Android/iOS have no ozky build — flag them so we don't mis-detect a desktop binary
	// (Android UA contains "Linux", iOS contains "Mac OS X").
	let mobileOs = $state(false);
	// Per-OS selected file-type index for the dropdowns.
	let sel = $state<Record<string, number>>({ macOS: 0, Windows: 0, Linux: 0 });

	let hero = $state<HTMLElement>();
	let section = $state<HTMLElement>();
	let track = $state<HTMLElement>();

	function assetFor(match: string): Asset | null {
		return assets.find((a) => a.name.toLowerCase().endsWith(match.toLowerCase())) ?? null;
	}

	// The detected platform's preferred (first resolvable) asset, for the hero box.
	let heroAsset = $derived.by<Asset | null>(() => {
		if (!userOs) return null;
		const p = downloads.platforms.find((p) => p.os === userOs);
		if (!p) return null;
		for (const t of p.types) {
			const a = assetFor(t.match);
			if (a) return a;
		}
		return null;
	});

	// Release assets that carry a sha256 digest, for the checksum list.
	let checksums = $derived(
		assets
			.filter((a) => a.digest && a.digest.startsWith('sha256:'))
			.map((a) => ({ name: a.name, sha: a.digest!.slice('sha256:'.length) }))
	);

	function scrollByCard(dir: 1 | -1) {
		if (!track) return;
		const card = track.querySelector('article');
		const w = card ? card.getBoundingClientRect().width : track.clientWidth / 3;
		const target = Math.max(
			0,
			Math.min(track.scrollWidth - track.clientWidth, track.scrollLeft + dir * w)
		);
		const proxy = { x: track.scrollLeft };
		gsap.to(proxy, {
			x: target,
			duration: 0.8,
			ease: 'power2.inOut',
			onUpdate: () => track && (track.scrollLeft = proxy.x)
		});
	}

	onMount(() => {
		const ua = navigator.userAgent;
		mobileOs = /Android|iPhone|iPad|iPod/i.test(ua);
		userOs = mobileOs
			? ''
			: /Mac/i.test(ua)
				? 'macOS'
				: /Win/i.test(ua)
					? 'Windows'
					: /Linux/i.test(ua)
						? 'Linux'
						: '';

		(async () => {
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
				// offline / rate-limited — hero + cards fall back to the releases page
			}
			loaded = true;
			ScrollTrigger.refresh();
		})();

		gsap.registerPlugin(ScrollTrigger);
		const tweens: gsap.core.Tween[] = [];

		// hero — reveal on load (about-01 field)
		if (hero) {
			const qh = (s: string) => Array.from(hero!.querySelectorAll<HTMLElement>(s));
			tweens.push(
				gsap.from(qh('[data-hero-title]'), { yPercent: 115, duration: 1.2, ease: 'power3.inOut' })
			);
			tweens.push(
				gsap.fromTo(
					qh('[data-hero-box]'),
					{ clipPath: 'inset(0 0 100% 0)' },
					{ clipPath: 'inset(0 0 0% 0)', duration: 1.2, ease: 'power2.inOut', delay: 0.15 }
				)
			);
			tweens.push(
				gsap.from(qh('[data-hero-sub]'), {
					y: 30,
					autoAlpha: 0,
					duration: 1.2,
					ease: 'power2.inOut',
					delay: 0.1
				})
			);
		}

		// scroll section — title wipe + card vectors
		if (section) {
			const q = (s: string) => Array.from(section!.querySelectorAll<HTMLElement>(s));
			tweens.push(
				gsap.fromTo(
					q('[data-title]'),
					{ clipPath: 'inset(0 100% 0 0)' },
					{
						clipPath: 'inset(0 0% 0 0)',
						duration: 1.2,
						ease: 'power2.inOut',
						scrollTrigger: { trigger: section, start: 'top 80%' }
					}
				)
			);
			q('[data-vector]').forEach((el) => {
				tweens.push(
					gsap.from(el, {
						scale: 0.55,
						autoAlpha: 0,
						duration: 1.2,
						ease: 'power2.inOut',
						transformOrigin: 'center',
						scrollTrigger: { trigger: el, start: 'top 88%' }
					})
				);
			});
		}

		ScrollTrigger.refresh();
		return () => tweens.forEach((t) => (t.scrollTrigger?.kill(), t.kill()));
	});
</script>

<svelte:head><title>ozky — Download</title></svelte:head>

<!-- HERO — about-01 layout: gold field, giant title, top-right download box -->
<section
	bind:this={hero}
	data-nav
	class="relative flex min-h-screen flex-col bg-gold px-8 pt-32 pb-8 text-ink lg:block lg:overflow-hidden"
>
	<!-- subtitle: version, centred -->
	<p
		data-hero-sub
		class="mx-auto mt-[6vh] max-w-[24ch] text-center font-display text-[clamp(1.2rem,2vw,1.8rem)] font-medium leading-snug lg:mt-[12vh]"
	>
		version: {loaded ? (version ?? 'unavailable') : 'checking…'}
	</p>

	<!-- top-right: direct download for the detected OS -->
	<div
		data-hero-box
		data-nav="light"
		class="order-3 mt-8 w-full bg-ink p-8 text-gold lg:absolute lg:top-24 lg:right-8 lg:mt-0 lg:w-[380px]"
	>
		<Tetra class="mb-6 h-12 w-12 text-gold" />
		{#if mobileOs}
			<h2 class="font-display text-[clamp(1.4rem,1.8vw,1.7rem)] font-medium leading-tight">
				No mobile build.
			</h2>
			<p class="mono mt-5 text-[11px] leading-[1.8] text-gold">
				ozky is a desktop wallet — there's no Android or iOS build. Open this page on macOS,
				Windows, or Linux to download.
			</p>
		{:else if userOs}
			<h2 class="font-display text-[clamp(1.4rem,1.8vw,1.7rem)] font-medium leading-tight">
				Download for {userOs}.
			</h2>
			<p class="mono mt-5 text-[11px] leading-[1.8] text-gold">{downloads.notice}</p>
			<a
				href={heroAsset?.browser_download_url ?? downloads.releasesUrl}
				download={heroAsset ? '' : undefined}
				class="mono mt-7 inline-flex w-fit items-center rounded-full border border-gold bg-gold px-7 py-3 text-[11px] leading-none text-ink transition-opacity duration-300 hover:opacity-80"
			>
				{loaded ? (heroAsset ? `Download ${heroAsset.name} ↓` : 'Get it on GitHub ↗') : 'Checking…'}
			</a>
		{:else}
			<h2 class="font-display text-[clamp(1.4rem,1.8vw,1.7rem)] font-medium leading-tight">
				Choose your platform below.
			</h2>
			<p class="mono mt-5 text-[11px] leading-[1.8] text-gold">{downloads.notice}</p>
		{/if}
	</div>

	<!-- giant title, bottom-left -->
	<h1
		class="order-2 mt-10 overflow-hidden font-display text-[clamp(3rem,16vw,15rem)] font-semibold leading-[0.8] tracking-[-0.04em] lg:absolute lg:bottom-4 lg:left-6 lg:mt-0"
	>
		<span data-hero-title class="block">{downloads.heading}</span>
	</h1>
</section>

<!-- SCROLL — all platforms, Solutions-style card track with file-type dropdowns -->
<section bind:this={section} class="bg-grey pt-16 text-ink">
	<div class="flex items-end justify-between gap-6 px-8 pb-10">
		<h2
			data-title
			class="font-display max-w-[16ch] text-[clamp(1.4rem,2.8vw,2.3rem)] leading-[1.05] font-normal tracking-[-0.02em]"
		>
			Every platform. Pick a build, pick a format.
		</h2>
		<div class="flex shrink-0 gap-2.5">
			<button
				onclick={() => scrollByCard(-1)}
				aria-label="Previous"
				class="grid h-[2.1rem] w-[2.1rem] place-items-center rounded-full bg-ink text-grey transition-colors hover:bg-grey hover:text-ink"
			>
				<svg viewBox="0 0 24 24" class="h-3 w-3" fill="none"
					><path d="M15 5 L8 12 L15 19" stroke="currentColor" stroke-width="2.4" /></svg
				>
			</button>
			<button
				onclick={() => scrollByCard(1)}
				aria-label="Next"
				class="grid h-[2.1rem] w-[2.1rem] place-items-center rounded-full bg-ink text-grey transition-colors hover:bg-grey hover:text-ink"
			>
				<svg viewBox="0 0 24 24" class="h-3 w-3" fill="none"
					><path d="M9 5 L16 12 L9 19" stroke="currentColor" stroke-width="2.4" /></svg
				>
			</button>
		</div>
	</div>

	<div
		bind:this={track}
		class="flex snap-x snap-mandatory overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
	>
		{#each downloads.platforms as p, i (p.os)}
			{@const G = graphics[p.os]}
			{@const asset = assetFor(p.types[sel[p.os]].match)}
			<article
				class="flex min-h-[72vh] w-[88vw] shrink-0 snap-start flex-col border border-ink bg-grey p-9 sm:w-[60vw] lg:w-[calc(100%/3)] {i >
				0
					? '-ml-px'
					: ''}"
			>
				<div class="flex items-center justify-between">
					<h3 class="font-display text-[clamp(1.4rem,2vw,1.9rem)] font-medium tracking-[-0.02em]">
						{p.os}
					</h3>
					{#if userOs === p.os}<span class="mono text-[10px] text-ink">Detected</span>{/if}
				</div>
				<p class="mono mt-2 text-[11px] text-ink">{p.note}</p>

				<div class="grid flex-1 place-items-center">
					<div data-vector>
						<G class="h-48 w-48 text-ink" />
					</div>
				</div>

				<!-- file-type dropdown -->
				<label class="mono block text-[10px] text-ink">
					File type
					<select
						bind:value={sel[p.os]}
						class="mono mt-2 w-full border border-ink bg-grey px-3 py-2 text-[11px] text-ink focus:ring-0"
					>
						{#each p.types as t, ti (t.match)}
							<option value={ti}>{t.label}</option>
						{/each}
					</select>
				</label>

				<a
					href={asset?.browser_download_url ?? downloads.releasesUrl}
					download={asset ? '' : undefined}
					class="mono mt-5 inline-flex w-fit items-center rounded-full border border-ink px-7 py-3 text-[11px] leading-none text-ink transition-colors duration-300 hover:bg-ink hover:text-grey"
				>
					{loaded ? (asset ? 'Download ↓' : 'Not in this release — GitHub ↗') : 'Checking…'}
				</a>
			</article>
		{/each}
	</div>
</section>

<!-- SHA-256 — checksums from the GitHub build -->
<section class="bg-ink px-8 py-24 text-grey">
	<h2 class="font-display text-[clamp(1.6rem,3vw,2.6rem)] font-medium tracking-[-0.02em]">
		SHA-256 checksums
	</h2>
	<p class="mono mt-4 max-w-[60ch] text-[11px] leading-[1.8] text-grey">
		Verify your download against the digest published with the {version ?? 'latest'} release build.
	</p>

	{#if loaded && checksums.length}
		<ul class="mono mt-10 space-y-5 text-[10px] leading-[1.7]">
			{#each checksums as c (c.name)}
				<li class="border-t border-grey pt-4">
					<p class="text-gold">{c.name}</p>
					<p class="mt-1 break-all text-grey">{c.sha}</p>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="mono mt-10 text-[11px] text-grey">
			{loaded
				? 'No digests published for this release yet — verify against the assets on GitHub.'
				: 'Loading checksums…'}
		</p>
	{/if}
</section>
