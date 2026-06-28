<script lang="ts">
	// System Status backdrop: a small grid of Aztec-rune glyphs. Each cell flips to a
	// new random rune on its OWN random schedule (0.5–3s) and fades in, so the field
	// shimmers asynchronously like decoding glyphs. Runes are masked to grey; an
	// occasional cell flashes gold. Decorative; pointer-events:none.
	//
	// `fill` makes it cover its (absolutely-positioned) parent — used by the Loader —
	// sizing the count to the viewport instead of the fixed 8-row band.
	import { onMount } from 'svelte';
	import { fade } from 'svelte/transition';

	let {
		fill = false,
		cell = 16,
		gap = 7,
		runeSize = 14
	}: { fill?: boolean; cell?: number; gap?: number; runeSize?: number } = $props();

	const RUNES = [
		'sym_back_c',
		'sym_circle',
		'sym_circle_in_circle',
		'sym_circle_v_line',
		'sym_circle_x_sq',
		'sym_cross_v',
		'sym_curved_x_line',
		'sym_half_e',
		'sym_hash',
		'sym_hourglass',
		'sym_line_arcs',
		'sym_psi',
		'sym_semicircle',
		'sym_sq_in_sq',
		'sym_square',
		'sym_topline_inv_v',
		'sym_v_cross_line',
		'sym_v_doublecross_line',
		'sym_window'
	];
	const rune = () => RUNES[(Math.random() * RUNES.length) | 0];
	const opacity = () => 0.28 + Math.random() * 0.38;
	const gold = () => Math.random() < 0.12;
	const nextDelay = () => 500 + Math.random() * 2500;

	let cells = $state<{ r: string; o: number; g: boolean }[]>([]);

	onMount(() => {
		// 8 rows by default; full-screen sizes to the viewport (capped for perf).
		let count = 160;
		if (fill) {
			const pitch = cell + gap;
			const cols = Math.ceil(window.innerWidth / pitch);
			const rows = Math.ceil(window.innerHeight / pitch);
			count = Math.min(cols * rows + cols, 800);
		}
		cells = Array.from({ length: count }, () => ({ r: rune(), o: opacity(), g: gold() }));
		const timers: ReturnType<typeof setTimeout>[] = [];
		const schedule = (i: number) => {
			timers[i] = setTimeout(() => {
				cells[i] = { r: rune(), o: opacity(), g: gold() };
				schedule(i);
			}, nextDelay());
		};
		for (let i = 0; i < count; i++) schedule(i);
		return () => timers.forEach(clearTimeout);
	});
</script>

<div
	class="field"
	class:fill
	style="--cell:{cell}px; --gap:{gap}px; --rune:{runeSize}px;"
	aria-hidden="true"
>
	{#each cells as cell, i (i)}
		<span class="cell">
			{#key cell.r}
				<span
					class="rune"
					in:fade={{ duration: 500 }}
					style="opacity:{cell.o}; background:{cell.g
						? 'var(--color-gold)'
						: 'var(--color-grey)'}; -webkit-mask-image:url(/runes/{cell.r}.svg); mask-image:url(/runes/{cell.r}.svg);"
				></span>
			{/key}
		</span>
	{/each}
</div>

<style>
	.field {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(var(--cell), 1fr));
		grid-auto-rows: var(--cell);
		gap: var(--gap);
		max-height: calc(8 * var(--cell) + 7 * var(--gap));
		overflow: hidden;
		pointer-events: none;
	}
	.field.fill {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		max-height: none;
		justify-content: center;
		align-content: center;
	}
	.cell {
		position: relative;
		display: grid;
		place-items: center;
	}
	.rune {
		position: absolute;
		width: var(--rune);
		height: var(--rune);
		-webkit-mask-repeat: no-repeat;
		mask-repeat: no-repeat;
		-webkit-mask-position: center;
		mask-position: center;
		-webkit-mask-size: contain;
		mask-size: contain;
	}
</style>
