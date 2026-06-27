<script lang="ts" module>
	let counter = 0;
	function nextId() {
		return ++counter;
	}
</script>

<script lang="ts">
	// Neutral abstract B&W placeholder block (no defense imagery from the reference).
	// Layered gradients + SVG fractal noise read as a grainy monochrome photo.
	let {
		class: cls = '',
		seed = 4,
		label = ''
	}: { class?: string; seed?: number; label?: string } = $props();

	// Stable id by render order (SSR + client agree — no hydration mismatch).
	const id = `noise-${nextId()}`;
</script>

<div class="relative overflow-hidden bg-[#3a3a3a] {cls}">
	<div
		class="absolute inset-0"
		style="background:
			radial-gradient(120% 90% at 30% 10%, #6b6b6b 0%, #2c2c2c 45%, #161616 100%),
			linear-gradient(200deg, rgba(255,255,255,0.06), rgba(0,0,0,0.5));"
	></div>
	<svg class="absolute inset-0 h-full w-full opacity-[0.5] mix-blend-overlay" aria-hidden="true">
		<filter {id}>
			<feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" {seed} />
			<feColorMatrix type="saturate" values="0" />
		</filter>
		<rect width="100%" height="100%" filter="url(#{id})" />
	</svg>
	{#if label}
		<span class="mono absolute bottom-3 left-3 text-[10px] text-paper/40">{label}</span>
	{/if}
</div>
