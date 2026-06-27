<script lang="ts">
	let { class: cls = '' }: { class?: string } = $props();
</script>

<svg viewBox="0 0 200 200" fill="none" class={cls} aria-hidden="true">
	<g stroke="currentColor" stroke-width="1" fill="none">
		<circle cx="100" cy="100" r="80" />
		<ellipse cx="100" cy="100" rx="80" ry="28" />
		<ellipse cx="100" cy="100" rx="80" ry="56" />
		<ellipse cx="100" cy="100" rx="28" ry="80" />
		<ellipse cx="100" cy="100" rx="56" ry="80" />
		<line x1="20" y1="100" x2="180" y2="100" />
		<line x1="100" y1="20" x2="100" y2="180" />
	</g>
</svg>
