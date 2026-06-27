<script lang="ts">
	let { class: cls = '' }: { class?: string } = $props();
</script>

<svg viewBox="0 0 200 200" fill="none" class={cls} aria-hidden="true">
	<g stroke="currentColor" stroke-width="1.1">
		<path d="M100 20 L20 165 L180 165 Z" />
		<path d="M100 20 L100 120 M20 165 L100 120 L180 165" />
		<path d="M100 120 L60 92 M100 120 L140 92" opacity="0.6" />
		<path d="M60 92 L140 92" opacity="0.4" />
	</g>
</svg>
