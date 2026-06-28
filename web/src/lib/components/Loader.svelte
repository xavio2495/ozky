<script lang="ts">
	// Initial-load cover: black screen, full-page rune-glitch, and the ozky lockup acting
	// as a left→right colour-fill loading bar. Load % is shown in all four corners.
	// Progress is rAF-driven (NOT GSAP) because GSAP playback is paused until this lifts —
	// on finish it calls releaseAnimations() so the page's intro tweens play on reveal.
	import { onMount } from 'svelte';
	import { releaseAnimations } from '$lib/scroll';
	import RuneGlitch from './home/RuneGlitch.svelte';

	let { onComplete }: { onComplete: () => void } = $props();

	let pct = $state(0);
	let hiding = $state(false);

	onMount(() => {
		const DUR = 1300;
		const start = performance.now();
		let raf = 0;
		let done = 0;

		const tick = (now: number) => {
			const p = Math.min(1, (now - start) / DUR);
			pct = Math.round(p * 100);
			if (p < 1) {
				raf = requestAnimationFrame(tick);
			} else {
				releaseAnimations(); // let the revealed page's intro tweens play
				hiding = true; // CSS fade-out
				done = window.setTimeout(onComplete, 550);
			}
		};
		raf = requestAnimationFrame(tick);

		return () => {
			cancelAnimationFrame(raf);
			clearTimeout(done);
		};
	});
</script>

<div
	class="fixed inset-0 z-[100] flex items-center justify-center overflow-hidden bg-ink transition-opacity duration-500"
	class:opacity-0={hiding}
	class:pointer-events-none={hiding}
>
	<!-- rune field across the whole screen (coarse cells = fast first paint) -->
	<RuneGlitch fill cell={42} gap={16} runeSize={26} />

	<!-- load % in the four corners -->
	<span class="mono pointer-events-none absolute top-6 left-6 text-[12px] text-gold">{pct}%</span>
	<span class="mono pointer-events-none absolute top-6 right-6 text-[12px] text-gold">{pct}%</span>
	<span class="mono pointer-events-none absolute bottom-6 left-6 text-[12px] text-gold">{pct}%</span
	>
	<span class="mono pointer-events-none absolute right-6 bottom-6 text-[12px] text-gold"
		>{pct}%</span
	>

	<!-- ozky lockup as a colour-fill loading bar (grey → gold, left → right) -->
	<div class="logo-bar relative w-[min(62vw,640px)]">
		<div class="absolute inset-0 bg-grey"></div>
		<div class="absolute inset-y-0 left-0 bg-gold" style:width="{pct}%"></div>
	</div>
</div>

<style>
	.logo-bar {
		aspect-ratio: 1003 / 255;
		-webkit-mask: url(/img/logo_with_icon.svg) center / contain no-repeat;
		mask: url(/img/logo_with_icon.svg) center / contain no-repeat;
	}
</style>
