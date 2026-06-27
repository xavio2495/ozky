<script lang="ts">
	import { page } from '$app/state';
	import { docsNav } from '$lib/content/docs';
	let { children } = $props();
</script>

<svelte:head><title>ozky — Docs</title></svelte:head>

<section class="min-h-screen bg-grey px-8 pt-32 pb-24 text-ink">
	<div class="grid grid-cols-1 gap-12 lg:grid-cols-[220px_1fr]">
		<!-- sidebar -->
		<aside class="lg:sticky lg:top-28 lg:self-start">
			<p class="mono mb-5 text-[10px] text-ink">Documentation</p>
			<nav class="flex flex-col gap-1">
				{#each docsNav as item (item.href)}
					<a
						href={item.href}
						class="font-display text-lg font-medium transition-colors hover:text-gold {page.url
							.pathname === item.href
							? 'text-gold'
							: 'text-ink'}"
					>
						{item.label}
					</a>
				{/each}
			</nav>
		</aside>

		<!-- content -->
		<div
			class="prose max-w-[68ch] prose-headings:font-display prose-headings:font-semibold prose-headings:tracking-[-0.02em] prose-a:text-gold prose-code:font-mono"
		>
			{@render children()}
		</div>
	</div>
</section>
