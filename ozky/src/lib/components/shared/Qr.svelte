<script lang="ts">
	import QRCode from 'qrcode';

	let { data, size = 168 }: { data: string; size?: number } = $props();

	let svg = $state('');
	$effect(() => {
		if (!data) {
			svg = '';
			return;
		}
		QRCode.toString(data, {
			type: 'svg',
			margin: 1,
			width: size,
			color: { dark: '#0a0a0a', light: '#ffffff' }
		})
			.then((s) => (svg = s))
			.catch(() => (svg = ''));
	});
</script>

<div class="qr" style="width:{size}px;height:{size}px;">
	{#if svg}
		<!-- eslint-disable-next-line svelte/no-at-html-tags -->
		{@html svg}
	{/if}
</div>

<style>
	.qr {
		display: grid;
		place-items: center;
		padding: 10px;
		border-radius: var(--radius-lg);
		background: #fff;
		box-shadow: 0 8px 30px -12px color-mix(in oklch, var(--primary) 40%, transparent);
	}
	.qr :global(svg) {
		width: 100%;
		height: 100%;
		display: block;
	}
</style>
