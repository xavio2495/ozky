<script lang="ts">
	// Onboarding-only backdrop: a full-bleed grid of aztec-rune glyphs. Each cell flips
	// to a new random rune on its OWN random schedule (0.5–3s), so the field shimmers
	// asynchronously like decoding glyphs rather than pulsing in unison. Each rune is
	// masked to the gold accent at low opacity. Purely decorative; pointer-events:none.
	import { onMount } from 'svelte';
	import { fade } from 'svelte/transition';

	const RUNES = [
		'sym_back_c', 'sym_circle', 'sym_circle_in_circle', 'sym_circle_v_line', 'sym_circle_x_sq',
		'sym_cross_v', 'sym_curved_x_line', 'sym_half_e', 'sym_hash', 'sym_hourglass', 'sym_line_arcs',
		'sym_psi', 'sym_semicircle', 'sym_sq_in_sq', 'sym_square', 'sym_topline_inv_v',
		'sym_v_cross_line', 'sym_v_doublecross_line', 'sym_window'
	];
	// Generous fixed count; the CSS grid auto-fills and clips overflow on any window size.
	const COUNT = 240;
	const rune = () => RUNES[(Math.random() * RUNES.length) | 0];
	const opacity = () => 0.05 + Math.random() * 0.1;
	// Random delay in ms before a cell next flips.
	const nextDelay = () => 500 + Math.random() * 2500;

	let cells = $state(Array.from({ length: COUNT }, () => ({ r: rune(), o: opacity() })));

	onMount(() => {
		const timers: ReturnType<typeof setTimeout>[] = [];
		const schedule = (i: number) => {
			timers[i] = setTimeout(() => {
				cells[i] = { r: rune(), o: opacity() };
				schedule(i);
			}, nextDelay());
		};
		for (let i = 0; i < COUNT; i++) schedule(i);
		return () => timers.forEach(clearTimeout);
	});
</script>

<div class="field" aria-hidden="true">
	{#each cells as cell, i (i)}
		<span class="cell">
			{#key cell.r}
				<span
					class="rune"
					in:fade={{ duration: 600 }}
					style="opacity:{cell.o}; -webkit-mask-image:url(/runes/{cell.r}.svg); mask-image:url(/runes/{cell.r}.svg);"
				></span>
			{/key}
		</span>
	{/each}
</div>

<style>
	.field {
		position: absolute;
		inset: 0;
		z-index: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(58px, 1fr));
		grid-auto-rows: 58px;
		gap: 10px;
		padding: 16px;
		overflow: hidden;
		pointer-events: none;
		background:
			radial-gradient(120% 80% at 50% 0%, color-mix(in oklch, var(--primary) 6%, transparent), transparent 60%),
			radial-gradient(55% 40% at 0% 100%, color-mix(in oklch, var(--primary) 6%, transparent), transparent 60%),
			radial-gradient(55% 40% at 100% 100%, color-mix(in oklch, var(--primary) 6%, transparent), transparent 60%),
			var(--background);
	}
	.cell {
		position: relative;
		display: grid;
		place-items: center;
	}
	.rune {
		position: absolute;
		width: 34px;
		height: 34px;
		background: var(--primary);
		-webkit-mask-repeat: no-repeat;
		mask-repeat: no-repeat;
		-webkit-mask-position: center;
		mask-position: center;
		-webkit-mask-size: contain;
		mask-size: contain;
	}
	/* A soft vignette so the centered card reads clearly over the field. */
	.field::after {
		content: '';
		position: absolute;
		inset: 0;
		background: radial-gradient(60% 50% at 50% 50%, color-mix(in oklch, var(--background) 78%, transparent), transparent 75%);
	}
</style>
