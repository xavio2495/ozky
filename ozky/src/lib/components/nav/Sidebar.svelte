<script lang="ts">
	import { page } from '$app/stores';
	import { crossfade } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import LayoutDashboardIcon from '@lucide/svelte/icons/layout-dashboard';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ArrowDownLeftIcon from '@lucide/svelte/icons/arrow-down-left';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import ScaleIcon from '@lucide/svelte/icons/scale';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import SplitIcon from '@lucide/svelte/icons/split';
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import ReceiptIcon from '@lucide/svelte/icons/receipt';
	import TrendingUpIcon from '@lucide/svelte/icons/trending-up';
	import LockIcon from '@lucide/svelte/icons/lock';
	import { Button } from '$lib/components/ui/button';
	import { wallet } from '$lib/wallet.svelte';
	import { toast } from 'svelte-sonner';
	import { errMessage } from '$lib/api';
	import AccountSwitcher from './AccountSwitcher.svelte';

	async function lock() {
		try {
			await wallet.lock();
		} catch (e) {
			toast.error('Could not lock', { description: errMessage(e) });
		}
	}

	const items = [
		{ href: '/', label: 'Dashboard', icon: LayoutDashboardIcon },
		{ href: '/send', label: 'Send', icon: ArrowUpRightIcon },
		{ href: '/split', label: 'Split', icon: SplitIcon },
		{ href: '/payroll', label: 'Payroll', icon: CalendarClockIcon, badge: true },
		{ href: '/receive', label: 'Receive', icon: ArrowDownLeftIcon },
		{ href: '/deposit', label: 'Deposit', icon: DownloadIcon },
		{ href: '/withdraw', label: 'Withdraw', icon: UploadIcon },
		{ href: '/transactions', label: 'Transactions', icon: ReceiptIcon },
		{ href: '/markets', label: 'Markets', icon: TrendingUpIcon },
		{ href: '/auditor', label: 'Auditor', icon: ScaleIcon },
		{ href: '/settings', label: 'Settings', icon: SettingsIcon }
	];

	const [send, receive] = crossfade({ duration: 280, easing: cubicOut });

	const isActive = (href: string) =>
		href === '/' ? $page.url.pathname === '/' : $page.url.pathname.startsWith(href);
</script>

<nav class="sidebar">
	<div class="brand">
		<img src="/brand/logo.svg" alt="ozky" class="h-[18px] w-auto opacity-90" />
	</div>

	<div class="mb-4">
		<AccountSwitcher />
	</div>

	<ul class="flex flex-col gap-1">
		{#each items as item (item.href)}
			{@const active = isActive(item.href)}
			<li class="relative">
				{#if active}
					<div
						class="pill"
						in:receive={{ key: 'pill' }}
						out:send={{ key: 'pill' }}
					></div>
				{/if}
				<a href={item.href} class="nav-item" data-active={active}>
					<item.icon class="size-[18px]" />
					<span>{item.label}</span>
					{#if item.badge && wallet.dueCount > 0}
						<span class="due-dot" title="{wallet.dueCount} due">{wallet.dueCount}</span>
					{/if}
				</a>
			</li>
		{/each}
	</ul>

	<div class="foot">
		<Button variant="outline" size="sm" class="w-full justify-start gap-2" onclick={lock}>
			<LockIcon class="size-4" />
			Lock wallet
		</Button>
		<div class="mt-2 flex items-center gap-2 px-1">
			<span class="size-1.5 rounded-full bg-primary/80"></span>
			<span class="text-xs text-muted-foreground">Shielded · v0.1</span>
		</div>
	</div>
</nav>

<style>
	.sidebar {
		display: flex;
		flex-direction: column;
		width: 232px;
		flex-shrink: 0;
		padding: 18px 14px;
		border-right: 1px solid var(--border);
		background: color-mix(in oklch, var(--card) 40%, transparent);
	}
	.brand {
		padding: 6px 10px 22px;
	}
	.nav-item {
		position: relative;
		z-index: 1;
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 10px 12px;
		border-radius: var(--radius-md);
		font-size: 0.875rem;
		font-weight: 500;
		color: var(--muted-foreground);
		transition: color 0.18s ease, transform 0.18s ease;
	}
	.nav-item:hover {
		color: var(--foreground);
		transform: translateX(2px);
	}
	.nav-item[data-active='true'] {
		color: var(--primary);
	}
	.due-dot {
		margin-left: auto;
		min-width: 18px;
		height: 18px;
		padding: 0 5px;
		display: grid;
		place-items: center;
		border-radius: 999px;
		background: var(--primary);
		color: var(--primary-foreground);
		font-size: 0.6875rem;
		font-weight: 600;
	}
	.pill {
		position: absolute;
		inset: 0;
		border-radius: var(--radius-md);
		background: color-mix(in oklch, var(--primary) 14%, transparent);
		box-shadow: inset 2px 0 0 var(--primary);
	}
	.foot {
		display: flex;
		flex-direction: column;
		margin-top: auto;
		padding: 10px 8px 2px;
	}
</style>
