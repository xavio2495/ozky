<script lang="ts">
	// Radial halftone dot-field: dots shrink toward the center ring.
	let { class: cls = '' }: { class?: string } = $props();

	const dots: { x: number; y: number; r: number }[] = [];
	const N = 26;
	for (let gx = 0; gx < N; gx++) {
		for (let gy = 0; gy < N; gy++) {
			const x = (gx + 0.5) * (200 / N);
			const y = (gy + 0.5) * (200 / N);
			const d = Math.hypot(x - 100, y - 100);
			if (d > 95) continue;
			// ring emphasis: largest around r~55, smaller at center + edge
			const t = Math.abs(d - 52) / 52;
			const r = Math.max(0.2, 3.4 * (1 - t));
			dots.push({ x, y, r });
		}
	}
</script>

<svg viewBox="0 0 200 200" class={cls} aria-hidden="true">
	{#each dots as d (`${d.x}-${d.y}`)}
		<circle cx={d.x} cy={d.y} r={d.r} fill="currentColor" />
	{/each}
</svg>
