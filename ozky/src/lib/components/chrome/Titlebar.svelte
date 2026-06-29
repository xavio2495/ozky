<script lang="ts">
	// Custom window chrome (the app runs frameless — decorations:false). The bar is the
	// drag region; the control buttons sit on top as the click targets.
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import MinusIcon from '@lucide/svelte/icons/minus';
	import SquareIcon from '@lucide/svelte/icons/square';
	import XIcon from '@lucide/svelte/icons/x';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import LockIcon from '@lucide/svelte/icons/lock';
	import NotificationCenter from '$lib/components/nav/NotificationCenter.svelte';
	import { goto } from '$app/navigation';
	import { wallet } from '$lib/wallet.svelte';
	import { toast } from 'svelte-sonner';
	import { errMessage } from '$lib/api';

	const appWindow = getCurrentWindow();

	// On macOS the window uses native controls (Overlay traffic lights, top-left — see
	// tauri.macos.conf.json), so we hide our custom min/max/close buttons there and let the
	// OS draw them; the action buttons (notifications/settings/lock) stay right-aligned.
	const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.userAgent);

	async function lock() {
		try {
			await wallet.lock();
		} catch (e) {
			toast.error('Could not lock', { description: errMessage(e) });
		}
	}
</script>

<header data-tauri-drag-region class="titlebar">
	<div class="drag-spacer"></div>

	<div class="right">
		{#if wallet.unlocked}
			<div class="actions">
				<NotificationCenter />
				<button class="ctl" aria-label="Settings" title="Settings" onclick={() => goto('/settings')}>
					<SettingsIcon class="size-3.5" />
				</button>
				<button class="ctl" aria-label="Lock wallet" title="Lock wallet" onclick={lock}>
					<LockIcon class="size-3.5" />
				</button>
			</div>
			<div class="sep"></div>
		{/if}

		{#if !isMac}
			<div class="controls">
				<button class="ctl" aria-label="Minimize" onclick={() => appWindow.minimize()}>
					<MinusIcon class="size-3.5" />
				</button>
				<button class="ctl" aria-label="Maximize" onclick={() => appWindow.toggleMaximize()}>
					<SquareIcon class="size-3" />
				</button>
				<button class="ctl close" aria-label="Close" onclick={() => appWindow.close()}>
					<XIcon class="size-3.5" />
				</button>
			</div>
		{/if}
	</div>
</header>

<style>
	.titlebar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		height: 38px;
		padding-left: 14px;
		padding-right: 6px;
		background: transparent;
		user-select: none;
	}
	.drag-spacer {
		flex: 1;
		align-self: stretch;
	}
	.right {
		display: flex;
		align-items: center;
		gap: 4px;
	}
	.actions {
		display: flex;
		gap: 2px;
	}
	.sep {
		width: 1px;
		height: 16px;
		margin: 0 4px;
		background: var(--border);
	}
	.controls {
		display: flex;
		gap: 2px;
	}
	.ctl {
		position: relative;
		display: grid;
		place-items: center;
		width: 30px;
		height: 26px;
		border-radius: 7px;
		color: var(--muted-foreground);
		transition: background 0.15s ease, color 0.15s ease;
	}
	.ctl:hover {
		background: var(--accent);
		color: var(--accent-foreground);
	}
	.ctl.close:hover {
		background: var(--destructive);
		color: white;
	}
</style>
