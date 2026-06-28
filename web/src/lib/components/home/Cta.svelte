<script lang="ts">
	import { onMount } from 'svelte';
	import Button from '../ui/Button.svelte';
	import { gsap, ScrollTrigger } from '$lib/scroll';
	import { cta } from '$lib/content/home';

	let section = $state<HTMLElement>();

	onMount(() => {
		if (!section) return;
		gsap.registerPlugin(ScrollTrigger);
		const el = section.querySelector<HTMLElement>('[data-cta]');
		if (!el) return;
		// text reveals left → right with a rise
		const tween = gsap.from(el, {
			clipPath: 'inset(0 100% 0 0)',
			y: 50,
			duration: 1.2,
			ease: 'power2.inOut',
			scrollTrigger: { trigger: section, start: 'top 75%' }
		});
		ScrollTrigger.refresh();
		return () => (tween.scrollTrigger?.kill(), tween.kill());
	});
</script>

<section
	bind:this={section}
	class="grid min-h-[70vh] place-items-center bg-gold px-8 py-24 text-ink"
>
	<div data-cta class="flex flex-col items-center gap-12">
		<h2
			class="font-display text-center text-[clamp(2.6rem,8vw,7rem)] font-semibold leading-[0.92] tracking-[-0.04em]"
		>
			{cta.lead}<br />fully shielded?
		</h2>
		<Button href={cta.download.href} variant="solid-dark">{cta.download.label}</Button>
	</div>
</section>
