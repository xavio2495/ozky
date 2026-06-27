<script lang="ts">
	// Right-aligned, transparent pill nav over the rune field. The logo lives in the
	// Titlebar; activity/settings/lock moved there too — this bar is just the 5
	// primary destinations + the account switcher. Active-pill crossfade preserved.
	import { page } from '$app/stores';
	import { crossfade } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import LayoutDashboardIcon from '@lucide/svelte/icons/layout-dashboard';
	import WalletIcon from '@lucide/svelte/icons/wallet';
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import ArrowLeftRightIcon from '@lucide/svelte/icons/arrow-left-right';
	import ReceiptIcon from '@lucide/svelte/icons/receipt';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import CheckIcon from '@lucide/svelte/icons/check';
	import AccountSwitcher from './AccountSwitcher.svelte';
	import { api, errMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let copied = $state(false);
	async function copyAddresses() {
		try {
			const [shielded, publicAddr] = await Promise.all([
				api.receiveAddress(),
				api.fundingAddress()
			]);
			await navigator.clipboard.writeText(
				`Shielded (receive): ${shielded}\nPublic (Stellar): ${publicAddr}`
			);
			copied = true;
			toast.success('Addresses copied', { description: 'Shielded + public address on the clipboard' });
			setTimeout(() => (copied = false), 1500);
		} catch (e) {
			toast.error('Could not copy addresses', { description: errMessage(e) });
		}
	}

	const items = [
		{ href: '/', label: 'Dashboard', icon: LayoutDashboardIcon },
		{ href: '/wallet', label: 'Wallet', icon: WalletIcon },
		{ href: '/payroll', label: 'Payroll', icon: CalendarClockIcon },
		{ href: '/swap', label: 'Swap', icon: ArrowLeftRightIcon },
		{ href: '/transactions', label: 'Transactions', icon: ReceiptIcon }
	];

	const [send, receive] = crossfade({ duration: 280, easing: cubicOut });

	const isActive = (href: string) =>
		href === '/' ? $page.url.pathname === '/' : $page.url.pathname.startsWith(href);
</script>

<nav class="topnav">
	<div class="left">
		<a href="/" class="brand" aria-label="ozky home">
			<img src="/brand/logo.svg" alt="ozky" class="h-9 w-auto rounded-md" />
		</a>
		<div class="acct"><AccountSwitcher /></div>
		<button
			class="copy-addr"
			title="Copy shielded + public address"
			aria-label="Copy addresses"
			onclick={copyAddresses}
		>
			{#if copied}<CheckIcon class="size-4 text-primary" />{:else}<CopyIcon class="size-4" />{/if}
		</button>
	</div>

	<div class="pillbar">
		{#each items as item (item.href)}
			{@const active = isActive(item.href)}
			<div class="pill-wrap">
				{#if active}
					<div class="pill" in:receive={{ key: 'navpill' }} out:send={{ key: 'navpill' }}></div>
				{/if}
				<a href={item.href} class="pill-link" data-active={active}>
					<item.icon class="size-4" />
					<span>{item.label}</span>
				</a>
			</div>
		{/each}
	</div>
</nav>

<style>
	.topnav {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
		flex-shrink: 0;
		padding: 8px 20px;
		background: transparent;
	}
	.left {
		display: flex;
		align-items: center;
		gap: 12px;
		min-width: 0;
	}
	.brand {
		display: inline-flex;
		flex-shrink: 0;
	}
	.pillbar {
		display: flex;
		align-items: center;
		gap: 2px;
		padding: 4px;
		border: 1px solid var(--border);
		border-radius: 9999px;
		background: color-mix(in oklch, var(--card) 50%, transparent);
	}
	.pill-wrap {
		position: relative;
	}
	.pill-link {
		position: relative;
		z-index: 1;
		display: inline-flex;
		align-items: center;
		gap: 8px;
		padding: 8px 16px;
		border-radius: 9999px;
		font-size: 0.875rem;
		font-weight: 500;
		white-space: nowrap;
		color: var(--muted-foreground);
		transition: color 0.18s ease;
	}
	.pill-link:hover {
		color: var(--foreground);
	}
	.pill-link[data-active='true'] {
		color: var(--primary-foreground);
	}
	.pill {
		position: absolute;
		inset: 0;
		border-radius: 9999px;
		background: var(--primary);
	}
	.acct {
		width: 180px;
	}
	.copy-addr {
		display: grid;
		place-items: center;
		width: 34px;
		height: 34px;
		flex-shrink: 0;
		border-radius: 9999px;
		border: 1px solid var(--border);
		background: color-mix(in oklch, var(--card) 50%, transparent);
		color: var(--muted-foreground);
		transition: border-color 0.15s ease, color 0.15s ease, background 0.15s ease;
	}
	.copy-addr:hover {
		color: var(--foreground);
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
	@media (max-width: 920px) {
		.pill-link span {
			display: none;
		}
	}
</style>
