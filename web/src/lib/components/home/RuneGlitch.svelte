<script lang="ts">
	// System Status backdrop: a small grid of Aztec-rune glyphs. Each cell flips to a
	// new random rune on its OWN random schedule (0.5–3s) and fades in, so the field
	// shimmers asynchronously like decoding glyphs. Runes are masked to grey; an
	// occasional cell flashes gold. Decorative; pointer-events:none.
	import { onMount } from 'svelte';
	import { fade } from 'svelte/transition';

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
	// 8 rows; auto-fill clips overflow columns on any panel width.
	const COUNT = 160;
	const rune = () => RUNES[(Math.random() * RUNES.length) | 0];
	const opacity = () => 0.28 + Math.random() * 0.38;
	const gold = () => Math.random() < 0.12;
	const nextDelay = () => 500 + Math.random() * 2500;

	let cells = $state<{ r: string; o: number; g: boolean }[]>([]);

	onMount(() => {
		cells = Array.from({ length: COUNT }, () => ({ r: rune(), o: opacity(), g: gold() }));
		const timers: ReturnType<typeof setTimeout>[] = [];
		const schedule = (i: number) => {
			timers[i] = setTimeout(() => {
				cells[i] = { r: rune(), o: opacity(), g: gold() };
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
		grid-template-columns: repeat(auto-fill, minmax(16px, 1fr));
		grid-auto-rows: 16px;
		gap: 7px;
		max-height: calc(8 * 16px + 7 * 7px);
		overflow: hidden;
		pointer-events: none;
	}
	.cell {
		position: relative;
		display: grid;
		place-items: center;
	}
	.rune {
		position: absolute;
		width: 14px;
		height: 14px;
		-webkit-mask-repeat: no-repeat;
		mask-repeat: no-repeat;
		-webkit-mask-position: center;
		mask-position: center;
		-webkit-mask-size: contain;
		mask-size: contain;
	}
</style>
