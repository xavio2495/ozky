<script lang="ts">
	// The dashboard's emphasis card as a carousel: slide 1 = real pending actions,
	// followed by rotating ozky feature promos (placeholder for future brand promos).
	import * as Carousel from '$lib/components/ui/carousel';
	import type { CarouselAPI } from '$lib/components/ui/carousel/context';
	import { Button } from '$lib/components/ui/button';
	import CheckCircle2Icon from '@lucide/svelte/icons/check-circle-2';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import ArrowLeftRightIcon from '@lucide/svelte/icons/arrow-left-right';
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import ScaleIcon from '@lucide/svelte/icons/scale';
	import Repeat2Icon from '@lucide/svelte/icons/repeat-2';
	import type { Component } from 'svelte';

	type Row = { label: string; href: string };
	let { pending = [] }: { pending?: Row[] } = $props();

	const shown = $derived(pending.slice(0, 4));
	const more = $derived(Math.max(0, pending.length - 4));

	// Auto-advance every 8s; no manual arrows.
	let api = $state<CarouselAPI>();
	$effect(() => {
		if (!api) return;
		const id = setInterval(() => api?.scrollNext(), 8000);
		return () => clearInterval(id);
	});

	type Promo = { icon: Component; title: string; body: string; href: string; cta: string };
	const promos: Promo[] = [
		{
			icon: ShieldCheckIcon,
			title: 'Fully shielded payments',
			body: 'Amount, sender, and receiver are hidden on-chain by default.',
			href: '/wallet',
			cta: 'Send privately'
		},
		{
			icon: ArrowLeftRightIcon,
			title: 'Shielded swaps',
			body: 'Swap assets in-pool through a private AMM — your identity stays hidden.',
			href: '/swap',
			cta: 'Open Swap'
		},
		{
			icon: CalendarClockIcon,
			title: 'Automated payroll',
			body: 'Schedule recurring private disbursements; a keeper submits them for you.',
			href: '/payroll',
			cta: 'Set up payroll'
		},
		{
			icon: Repeat2Icon,
			title: 'Merchant channels',
			body: 'Let merchants pull up to a hidden per-period cap — cancel anytime.',
			href: '/subscriptions',
			cta: 'Explore channels'
		},
		{
			icon: ScaleIcon,
			title: 'Selective disclosure',
			body: 'Share a verifiable statement with an auditor for a chosen epoch range.',
			href: '/auditor',
			cta: 'Share a statement'
		}
	];
</script>

<Carousel.Root class="flex h-full flex-col" opts={{ align: 'start', loop: true }} setApi={(a) => (api = a)}>
	<Carousel.Content class="h-full">
		<!-- Real pending actions -->
		<Carousel.Item class="h-full">
			<div class="slide">
				<div class="head">
					<h3 class="title">Pending</h3>
					<span class="count">{pending.length}</span>
				</div>
				{#if pending.length === 0}
					<div class="caught-up">
						<CheckCircle2Icon class="size-5 text-primary" />
						<span>All caught up.</span>
					</div>
				{:else}
					<ul class="list">
						{#each shown as r (r.label)}
							<li>
								<a href={r.href} class="row">
									<span class="dot"></span>
									<span class="flex-1 truncate">{r.label}</span>
								</a>
							</li>
						{/each}
						{#if more > 0}<li class="more">+{more} more</li>{/if}
					</ul>
				{/if}
			</div>
		</Carousel.Item>

		<!-- Feature promos -->
		{#each promos as p (p.title)}
			<Carousel.Item class="h-full">
				<div class="slide promo">
					<span class="promo-ico"><p.icon class="size-5" /></span>
					<h3 class="title">{p.title}</h3>
					<p class="promo-body">{p.body}</p>
					<Button href={p.href} size="sm" variant="outline" class="mt-auto w-fit">{p.cta}</Button>
				</div>
			</Carousel.Item>
		{/each}
	</Carousel.Content>
</Carousel.Root>

<style>
	.slide {
		display: flex;
		flex-direction: column;
		gap: 10px;
		height: 100%;
		min-height: 150px;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.title {
		font-family: var(--font-heading);
		font-size: 0.9375rem;
		font-weight: 600;
		color: var(--primary);
	}
	.count {
		display: grid;
		place-items: center;
		min-width: 22px;
		height: 22px;
		padding: 0 7px;
		border-radius: 9999px;
		background: var(--primary);
		color: var(--primary-foreground);
		font-size: 0.6875rem;
		font-weight: 600;
	}
	.caught-up {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
	.list {
		display: flex;
		flex-direction: column;
		gap: 3px;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 9px;
		padding: 6px 8px;
		border-radius: var(--radius-lg);
		font-size: 0.8125rem;
		transition: background 0.12s ease;
	}
	.row:hover {
		background: color-mix(in oklch, var(--primary) 14%, transparent);
	}
	.dot {
		width: 7px;
		height: 7px;
		flex-shrink: 0;
		border-radius: 9999px;
		background: var(--primary);
	}
	.more {
		padding: 3px 8px;
		font-size: 0.6875rem;
		color: var(--muted-foreground);
	}
	.promo {
		gap: 8px;
	}
	.promo-ico {
		display: grid;
		place-items: center;
		width: 38px;
		height: 38px;
		border-radius: var(--radius-xl);
		background: color-mix(in oklch, var(--primary) 18%, transparent);
		color: var(--primary);
	}
	.promo-body {
		font-size: 0.8125rem;
		color: var(--muted-foreground);
		line-height: 1.4;
	}
</style>
