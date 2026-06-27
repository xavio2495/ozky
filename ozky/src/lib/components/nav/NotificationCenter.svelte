<script lang="ts">
	// The notification center: the titlebar bell, a compact pop-up preview, and a full
	// slide-over sidebar. Client-side — derived from the wallet's due/action items
	// (needs-attention) and its activity log (history). No backend event store.
	import { fly, fade } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import { goto } from '$app/navigation';
	import * as Popover from '$lib/components/ui/popover';
	import { Button } from '$lib/components/ui/button';
	import BellIcon from '@lucide/svelte/icons/bell';
	import XIcon from '@lucide/svelte/icons/x';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import RepeatIcon from '@lucide/svelte/icons/repeat';
	import HandCoinsIcon from '@lucide/svelte/icons/hand-coins';
	import CableIcon from '@lucide/svelte/icons/cable';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ArrowDownToLineIcon from '@lucide/svelte/icons/arrow-down-to-line';
	import ArrowUpFromLineIcon from '@lucide/svelte/icons/arrow-up-from-line';
	import ArrowLeftRightIcon from '@lucide/svelte/icons/arrow-left-right';
	import SplitIcon from '@lucide/svelte/icons/split';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import ScaleIcon from '@lucide/svelte/icons/scale';
	import ReceiptIcon from '@lucide/svelte/icons/receipt';
	import CheckCheckIcon from '@lucide/svelte/icons/check-check';
	import type { Component } from 'svelte';
	import { wallet, type Activity } from '$lib/wallet.svelte';

	let popOpen = $state(false);
	let sidebarOpen = $state(false);

	type NotifAction = {
		id: string;
		title: string;
		sub: string;
		href: string;
		icon: Component;
		tone: 'attention' | 'ready';
	};

	// Needs-attention items, derived from the live wallet state (mirrors the bell badge).
	const actions = $derived.by<NotifAction[]>(() => {
		const out: NotifAction[] = [];
		for (const p of wallet.payrolls)
			if (p.due)
				out.push({
					id: `payroll-${p.id}`,
					title: 'Payroll due',
					sub: p.label,
					href: '/payroll',
					icon: CalendarClockIcon,
					tone: 'attention'
				});
		for (const s of wallet.subscriptions)
			if (s.due)
				out.push({
					id: `sub-${s.id}`,
					title: 'Subscription due',
					sub: s.label,
					href: '/payroll',
					icon: RepeatIcon,
					tone: 'attention'
				});
		for (const e of wallet.escrows) {
			if (e.releasable)
				out.push({
					id: `escrow-rel-${e.id}`,
					title: 'Escrow ready to release',
					sub: `#${e.id} · ${e.asset}`,
					href: '/payroll',
					icon: HandCoinsIcon,
					tone: 'ready'
				});
			if (e.refundable)
				out.push({
					id: `escrow-ref-${e.id}`,
					title: 'Refund available',
					sub: `Escrow #${e.id} · ${e.asset}`,
					href: '/payroll',
					icon: HandCoinsIcon,
					tone: 'attention'
				});
		}
		for (const c of wallet.channels) {
			if (c.closeable)
				out.push({
					id: `chan-close-${c.id}`,
					title: 'Channel ready to collect',
					sub: `#${c.id} · ${c.asset}`,
					href: '/payroll',
					icon: CableIcon,
					tone: 'ready'
				});
			if (c.reclaimable)
				out.push({
					id: `chan-recl-${c.id}`,
					title: 'Channel reclaimable',
					sub: `#${c.id} · ${c.asset}`,
					href: '/payroll',
					icon: CableIcon,
					tone: 'attention'
				});
		}
		return out;
	});

	const total = $derived(actions.length);

	const ACTIVITY_ICONS: Record<Activity['kind'], Component> = {
		deposit: ArrowDownToLineIcon,
		send: ArrowUpRightIcon,
		split: SplitIcon,
		payroll: CalendarClockIcon,
		subscription: RepeatIcon,
		escrow: HandCoinsIcon,
		channel: CableIcon,
		withdraw: ArrowUpFromLineIcon,
		swap: ArrowLeftRightIcon,
		enroll: ShieldCheckIcon,
		disclose: ScaleIcon
	};

	function relTime(ts: number): string {
		const s = Math.floor((Date.now() - ts) / 1000);
		if (s < 45) return 'just now';
		const m = Math.floor(s / 60);
		if (m < 60) return `${m}m ago`;
		const h = Math.floor(m / 60);
		if (h < 24) return `${h}h ago`;
		const d = Math.floor(h / 24);
		if (d < 7) return `${d}d ago`;
		return new Date(ts).toLocaleDateString();
	}

	function openAction(href: string) {
		popOpen = false;
		sidebarOpen = false;
		goto(href);
	}

	function openSidebar() {
		popOpen = false;
		sidebarOpen = true;
	}
</script>

<Popover.Root bind:open={popOpen}>
	<Popover.Trigger
		class="relative grid h-[26px] w-[30px] place-items-center rounded-[7px] text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
		aria-label="Notifications"
		title={total > 0 ? `${total} item${total === 1 ? '' : 's'} need attention` : 'Notifications'}
	>
		<BellIcon class="size-3.5" />
		{#if total > 0}<span class="count-dot">{total}</span>{/if}
	</Popover.Trigger>
	<Popover.Content class="w-[360px] overflow-hidden p-0" align="end" sideOffset={8}>
		<div class="pop">
			<div class="pop-head">
				<span class="pop-title">Notifications</span>
				{#if total > 0}<span class="pop-count">{total}</span>{/if}
			</div>

			{#if actions.length === 0 && wallet.activity.length === 0}
				<div class="empty">
					<CheckCheckIcon class="size-5 text-muted-foreground" />
					<span>You're all caught up</span>
				</div>
			{:else}
				<div class="pop-body">
					{#if actions.length > 0}
						<div class="sec-label">Needs attention</div>
						{#each actions.slice(0, 4) as a (a.id)}
							<button class="item action" onclick={() => openAction(a.href)}>
								<span class="i-icon" data-tone={a.tone}><a.icon class="size-3.5" /></span>
								<span class="i-text">
									<span class="i-title">{a.title}</span>
									<span class="i-sub">{a.sub}</span>
								</span>
								<ChevronRightIcon class="size-3.5 text-muted-foreground" />
							</button>
						{/each}
					{/if}

					{#if wallet.activity.length > 0}
						<div class="sec-label">Recent activity</div>
						{#each wallet.activity.slice(0, 4) as ev (ev.id)}
							{@const Icon = ACTIVITY_ICONS[ev.kind]}
							<div class="item">
								<span class="i-icon"><Icon class="size-3.5" /></span>
								<span class="i-text">
									<span class="i-title">{ev.label}</span>
									{#if ev.detail}<span class="i-sub">{ev.detail}</span>{/if}
								</span>
								<span class="i-time">{relTime(ev.ts)}</span>
							</div>
						{/each}
					{/if}
				</div>
			{/if}

			<button class="pop-foot" onclick={openSidebar}>View all notifications</button>
		</div>
	</Popover.Content>
</Popover.Root>

{#if sidebarOpen}
	<button class="detail-scrim" aria-label="Close notifications" transition:fade={{ duration: 180 }} onclick={() => (sidebarOpen = false)}></button>
	<aside class="detail-panel" transition:fly={{ x: 420, duration: 260, easing: cubicOut }}>
		<div class="panel-head">
			<div class="drawer-head">
				<span class="ico"><BellIcon class="size-4 text-primary" /></span>
				<div>
					<div class="panel-title">Notifications</div>
					<div class="panel-sub">{total > 0 ? `${total} need attention` : 'All caught up'}</div>
				</div>
			</div>
			<button class="panel-close" aria-label="Close" onclick={() => (sidebarOpen = false)}>
				<XIcon class="size-4" />
			</button>
		</div>

		<div class="sb-body">
			{#if actions.length === 0 && wallet.activity.length === 0}
				<div class="empty tall">
					<CheckCheckIcon class="size-6 text-muted-foreground" />
					<span>You're all caught up</span>
					<p>Due payrolls, escrow releases, and recent activity will show up here.</p>
				</div>
			{:else}
				{#if actions.length > 0}
					<div class="sec-label">Needs attention</div>
					{#each actions as a (a.id)}
						<button class="item action lg" onclick={() => openAction(a.href)}>
							<span class="i-icon" data-tone={a.tone}><a.icon class="size-4" /></span>
							<span class="i-text">
								<span class="i-title">{a.title}</span>
								<span class="i-sub">{a.sub}</span>
							</span>
							<ChevronRightIcon class="size-4 text-muted-foreground" />
						</button>
					{/each}
				{/if}

				{#if wallet.activity.length > 0}
					<div class="sec-label">Activity</div>
					{#each wallet.activity as ev (ev.id)}
						{@const Icon = ACTIVITY_ICONS[ev.kind]}
						<div class="item lg">
							<span class="i-icon"><Icon class="size-4" /></span>
							<span class="i-text">
								<span class="i-title">{ev.label}</span>
								{#if ev.detail}<span class="i-sub">{ev.detail}</span>{/if}
							</span>
							<span class="i-time">{relTime(ev.ts)}</span>
						</div>
					{/each}
				{/if}
			{/if}
		</div>

		<div class="panel-foot">
			<Button variant="outline" class="flex-1" onclick={() => openAction('/transactions')}>
				<ReceiptIcon data-icon="inline-start" />
				Open transactions
			</Button>
		</div>
	</aside>
{/if}

<style>
	.count-dot {
		position: absolute;
		top: 1px;
		right: 1px;
		min-width: 14px;
		height: 14px;
		padding: 0 3px;
		display: grid;
		place-items: center;
		border-radius: 999px;
		background: var(--primary);
		color: var(--primary-foreground);
		font-size: 0.5625rem;
		font-weight: 600;
		line-height: 1;
	}

	/* ---- pop-up ---------------------------------------------------------- */
	.pop {
		display: flex;
		flex-direction: column;
	}
	.pop-head {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 12px 14px;
		border-bottom: 1px solid var(--border);
	}
	.pop-title {
		font-family: var(--font-heading);
		font-size: 0.875rem;
		font-weight: 600;
	}
	.pop-count {
		display: grid;
		place-items: center;
		min-width: 18px;
		height: 18px;
		padding: 0 5px;
		border-radius: 999px;
		background: var(--primary);
		color: var(--primary-foreground);
		font-size: 0.6875rem;
		font-weight: 600;
	}
	.pop-body {
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding: 6px;
		max-height: 360px;
		overflow-y: auto;
	}
	.sec-label {
		padding: 8px 8px 4px;
		font-size: 0.625rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--muted-foreground);
	}
	.item {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		padding: 8px 8px;
		border-radius: var(--radius-md);
		text-align: left;
	}
	.item.action {
		cursor: pointer;
		transition: background 0.14s ease;
	}
	.item.action:hover {
		background: var(--accent);
	}
	.i-icon {
		display: grid;
		place-items: center;
		flex-shrink: 0;
		width: 28px;
		height: 28px;
		border-radius: 999px;
		background: var(--muted);
		color: var(--muted-foreground);
	}
	.i-icon[data-tone='attention'] {
		background: color-mix(in oklch, var(--primary) 16%, transparent);
		color: var(--primary);
	}
	.i-icon[data-tone='ready'] {
		background: color-mix(in oklch, var(--primary) 10%, transparent);
		color: var(--primary);
	}
	.i-text {
		display: flex;
		flex-direction: column;
		min-width: 0;
		flex: 1;
	}
	.i-title {
		font-size: 0.8125rem;
		font-weight: 500;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.i-sub {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.i-time {
		flex-shrink: 0;
		font-size: 0.625rem;
		color: var(--muted-foreground);
	}
	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 8px;
		padding: 28px 16px;
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
	.empty.tall {
		padding: 60px 24px;
		text-align: center;
	}
	.empty.tall p {
		font-size: 0.75rem;
		line-height: 1.4;
		max-width: 260px;
	}
	.pop-foot {
		padding: 10px 14px;
		border-top: 1px solid var(--border);
		font-size: 0.75rem;
		font-weight: 500;
		color: var(--primary);
		text-align: center;
		transition: background 0.14s ease;
	}
	.pop-foot:hover {
		background: var(--accent);
	}

	/* ---- sidebar — same format as the transactions preview panel --------- */
	.detail-scrim {
		position: fixed;
		inset: 38px 0 0 0;
		z-index: 58;
		background: color-mix(in oklch, black 32%, transparent);
		backdrop-filter: blur(1px);
	}
	.detail-panel {
		position: fixed;
		top: 50px;
		right: 12px;
		bottom: 12px;
		z-index: 59;
		width: 400px;
		max-width: calc(100vw - 24px);
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border);
		border-radius: var(--radius-2xl);
		background: color-mix(in oklch, var(--card) 82%, transparent);
		backdrop-filter: blur(20px);
		-webkit-backdrop-filter: blur(20px);
		box-shadow: 0 16px 48px -16px rgb(0 0 0 / 0.7);
		overflow: hidden;
	}
	.panel-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		padding: 16px 16px 12px;
		border-bottom: 1px solid var(--border);
	}
	.drawer-head {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.ico {
		display: grid;
		place-items: center;
		width: 34px;
		height: 34px;
		flex-shrink: 0;
		border-radius: 999px;
		background: var(--muted);
	}
	.panel-title {
		font-family: var(--font-heading);
		font-size: 1rem;
		font-weight: 600;
	}
	.panel-sub {
		font-size: 0.75rem;
		color: var(--muted-foreground);
	}
	.panel-close {
		display: grid;
		place-items: center;
		width: 30px;
		height: 30px;
		flex-shrink: 0;
		border-radius: var(--radius-md);
		color: var(--muted-foreground);
	}
	.panel-close:hover {
		color: var(--foreground);
		background: color-mix(in oklch, var(--foreground) 8%, transparent);
	}
	.sb-body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding: 6px 8px 12px;
	}
	.item.lg {
		padding: 10px 8px;
	}
	.item.lg + .item.lg {
		border-top: 1px solid color-mix(in oklch, var(--border) 60%, transparent);
		border-radius: 0;
	}
	.panel-foot {
		display: flex;
		gap: 8px;
		padding: 12px 16px 16px;
		border-top: 1px solid var(--border);
	}
</style>
