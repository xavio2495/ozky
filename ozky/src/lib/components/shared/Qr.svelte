<script lang="ts">
	import QRCode from 'qrcode';

	let {
		data,
		size = 168,
		fg = '#0a0a0a',
		bg = '#ffffff',
		themed = false
	}: { data: string; size?: number; fg?: string; bg?: string; themed?: boolean } = $props();

	// Themed = gold modules on near-black, to match the app skin. Default stays
	// classic dark-on-white for maximum scannability.
	const dark = $derived(themed ? '#e8c34a' : fg);
	const light = $derived(themed ? '#0d0d0d' : bg);

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
			color: { dark, light }
		})
			.then((s) => (svg = s))
			.catch(() => (svg = ''));
	});
</script>

<div class="qr" class:themed style="width:{size}px;height:{size}px;">
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
		/* box-shadow: 0 8px 30px -12px color-mix(in oklch, var(--primary) 40%, transparent); */
	}
	.qr.themed {
		background: #0d0d0d;
		border: 1px solid var(--primary);
	}
	.qr :global(svg) {
		width: 100%;
		height: 100%;
		display: block;
	}
</style>
