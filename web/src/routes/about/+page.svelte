<script lang="ts">
	import { onMount } from 'svelte';
	import Tetra from '$lib/components/graphics/Tetra.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import { gsap, ScrollTrigger } from '$lib/scroll';
	import { about } from '$lib/content/about';
	import { social } from '$lib/content/site';

	let root = $state<HTMLElement>();

	// Contact form — no backend; composes a mailto so the message actually sends.
	let name = $state('');
	let email = $state('');
	let message = $state('');
	let sent = $state(false);

	function submit(e: SubmitEvent) {
		e.preventDefault();
		const subject = encodeURIComponent(`ozky — message from ${name || 'someone'}`);
		const body = encodeURIComponent(`${message}\n\n— ${name}${email ? ` <${email}>` : ''}`);
		window.location.href = `mailto:${social.email}?subject=${subject}&body=${body}`;
		sent = true;
	}

	onMount(() => {
		if (!root) return;
		gsap.registerPlugin(ScrollTrigger);
		const host = root;
		const q = (s: string) => Array.from(host.querySelectorAll<HTMLElement>(s));
		const tweens: gsap.core.Tween[] = [];

		// giant title fills upward behind its mask, on load
		tweens.push(
			gsap.from(q('[data-giant]'), { yPercent: 115, duration: 1.2, ease: 'power3.inOut' })
		);
		// the dark info box wipes its colour in from top → bottom, on load
		tweens.push(
			gsap.fromTo(
				q('[data-fill]'),
				{ clipPath: 'inset(0 0 100% 0)' },
				{ clipPath: 'inset(0 0 0% 0)', duration: 1.2, ease: 'power2.inOut', delay: 0.15 }
			)
		);
		// everything else rises in once on screen
		q('[data-rise]').forEach((el) => {
			tweens.push(
				gsap.from(el, {
					y: 40,
					autoAlpha: 0,
					duration: 1.2,
					ease: 'power2.inOut',
					scrollTrigger: { trigger: el, start: 'top 88%' }
				})
			);
		});

		ScrollTrigger.refresh();
		return () => tweens.forEach((t) => (t.scrollTrigger?.kill(), t.kill()));
	});
</script>

<svelte:head><title>ozky — About</title></svelte:head>

<div bind:this={root}>
	<!-- hero -->
	<section class="relative min-h-screen overflow-hidden bg-gold px-8 pt-32 pb-8 text-ink">
		<p
			data-rise
			class="mx-auto mt-[10vh] max-w-[20ch] text-center font-display text-[clamp(1.2rem,2vw,1.8rem)] font-medium leading-snug"
		>
			{about.hero}
		</p>

		<div data-fill class="absolute top-24 right-8 hidden w-[360px] bg-ink p-8 text-gold lg:block">
			<Tetra class="mb-6 h-12 w-12 text-gold" />
			<h2 class="font-display text-[clamp(1.4rem,1.8vw,1.7rem)] font-medium leading-tight">
				{about.mission.title}
			</h2>
			<p class="mono mt-5 text-[11px] leading-[1.8] text-gold">{about.mission.paras[0]}</p>
		</div>

		<h1
			class="absolute bottom-4 left-6 overflow-hidden font-display text-[clamp(4rem,16vw,15rem)] font-semibold leading-[0.8] tracking-[-0.04em]"
		>
			<span data-giant class="block">{about.giant}</span>
		</h1>
	</section>

	<!-- mission -->
	<section class="bg-ink px-8 py-28 text-grey">
		<div class="grid grid-cols-1 gap-12 lg:grid-cols-[1fr_1.4fr]">
			<h2
				data-rise
				class="font-display text-[clamp(1.8rem,3vw,3rem)] font-medium leading-tight tracking-[-0.02em]"
			>
				{about.mission.title}
			</h2>
			<div data-rise class="mono space-y-6 text-[12px] leading-[1.9] text-grey">
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
			data-rise
			class="font-display max-w-[16ch] text-[clamp(1.8rem,3.4vw,3rem)] font-medium leading-[1.04] tracking-[-0.02em]"
		>
			{about.how.title}
		</h2>
		<div class="mt-12 grid grid-cols-1 gap-px bg-ink md:grid-cols-2">
			{#each about.how.points as p (p.k)}
				<div data-rise class="bg-grey p-8">
					<h3 class="font-display text-xl font-medium">{p.k}</h3>
					<p class="mono mt-3 max-w-[44ch] text-[11px] leading-[1.8] text-ink">{p.v}</p>
				</div>
			{/each}
		</div>
	</section>

	<!-- contact -->
	<section class="bg-ink px-8 py-28 text-grey">
		<div class="grid grid-cols-1 gap-14 lg:grid-cols-[1fr_1.2fr]">
			<div data-rise>
				<h2
					class="font-display text-[clamp(2rem,4vw,3.4rem)] font-medium leading-[1.02] tracking-[-0.03em]"
				>
					{about.contact.title}
				</h2>
				<p class="mono mt-6 max-w-[36ch] text-[11px] leading-[1.8] text-grey">
					{about.contact.body}
				</p>
				<div class="mono mt-8 space-y-2 text-[11px]">
					<p>
						<a href={`mailto:${social.email}`} class="text-gold hover:opacity-70">{social.email}</a>
					</p>
					<p><a href={social.github} class="text-grey hover:text-gold">GitHub ↗</a></p>
					<p><a href={social.telegram} class="text-grey hover:text-gold">Telegram ↗</a></p>
				</div>
			</div>

			<form data-rise class="space-y-10" onsubmit={submit}>
				<div class="grid grid-cols-1 gap-10 sm:grid-cols-2">
					<label class="block">
						<span class="mono text-[10px] text-grey">{about.contact.fields.name}</span>
						<input
							type="text"
							bind:value={name}
							required
							class="mt-2 w-full border-0 border-b border-grey bg-transparent px-0 pb-2 font-display text-lg text-grey focus:border-gold focus:ring-0"
						/>
					</label>
					<label class="block">
						<span class="mono text-[10px] text-grey">{about.contact.fields.email}</span>
						<input
							type="email"
							bind:value={email}
							required
							class="mt-2 w-full border-0 border-b border-grey bg-transparent px-0 pb-2 font-display text-lg text-grey focus:border-gold focus:ring-0"
						/>
					</label>
				</div>
				<label class="block">
					<span class="mono text-[10px] text-grey">{about.contact.fields.message}</span>
					<textarea
						rows="3"
						bind:value={message}
						required
						class="mt-2 w-full resize-none border-0 border-b border-grey bg-transparent px-0 pb-2 font-display text-lg text-grey focus:border-gold focus:ring-0"
					></textarea>
				</label>
				<div class="flex flex-wrap items-center gap-5">
					<Button variant="solid-light">{about.contact.submit}</Button>
					{#if sent}
						<p class="mono text-[11px] text-gold">
							Opening your mail app — thanks for reaching out.
						</p>
					{/if}
				</div>
			</form>
		</div>
	</section>
</div>
