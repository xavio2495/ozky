<script lang="ts">
	// The default charcoal card every page floats over the rune field (design-language §4).
	// Optional header (title + jump affordance); body via children.
	import type { Snippet } from 'svelte';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';

	let {
		title,
		href,
		emphasis = false,
		class: cls = '',
		children
	}: {
		title?: string;
		/** When set, a circular jump button (↗) drills into this route. */
		href?: string;
		/** Gold-tinted emphasis surface — at most one per page (design-language §5). */
		emphasis?: boolean;
		class?: string;
		children: Snippet;
	} = $props();
</script>

<section class="surface {cls}" class:emphasis>
	{#if title || href}
		<header class="head">
			{#if title}<h3 class="title">{title}</h3>{/if}
			{#if href}
				<a class="jump" {href} aria-label="Open {title ?? 'detail'}">
					<ArrowUpRightIcon class="size-4" />
				</a>
			{/if}
		</header>
	{/if}
	{@render children()}
</section>

<style>
	.surface {
		display: flex;
		flex-direction: column;
		padding: 22px;
		border: 1px solid var(--border);
		border-radius: var(--radius-3xl);
		background: var(--card);
		backdrop-filter: blur(16px);
		-webkit-backdrop-filter: blur(16px);
		box-shadow:
			0 1px 0 0 color-mix(in oklch, white 4%, transparent) inset,
			0 8px 24px -12px rgb(0 0 0 / 0.6);
	}
	.surface.emphasis {
		border-color: color-mix(in oklch, var(--primary) 25%, var(--border));
		background: color-mix(in oklch, var(--primary) 10%, var(--card));
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-bottom: 16px;
	}
	.title {
		font-family: var(--font-heading);
		font-size: 1rem;
		font-weight: 600;
	}
	.emphasis .title {
		color: var(--primary);
	}
	.jump {
		display: grid;
		place-items: center;
		width: 30px;
		height: 30px;
		flex-shrink: 0;
		border: 1px solid var(--border);
		border-radius: 9999px;
		color: var(--muted-foreground);
		transition: color 0.15s ease, border-color 0.15s ease;
	}
	.jump:hover {
		color: var(--primary);
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
</style>
