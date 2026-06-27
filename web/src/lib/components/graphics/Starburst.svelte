<script lang="ts">
	let { class: cls = '' }: { class?: string } = $props();

	const lines = Array.from({ length: 36 }, (_, i) => {
		const a = (i / 36) * Math.PI * 2;
		return { x: 100 + Math.cos(a) * 88, y: 100 + Math.sin(a) * 88 };
	});
</script>

<svg viewBox="0 0 200 200" fill="none" class={cls} aria-hidden="true">
	<g stroke="currentColor" stroke-width="0.9">
		{#each lines as l (`${l.x}-${l.y}`)}
			<line x1="100" y1="100" x2={l.x} y2={l.y} />
		{/each}
	</g>
</svg>
