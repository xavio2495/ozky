<script lang="ts">
	// Custom window chrome (the app runs frameless — decorations:false). The bar is the
	// drag region; the control buttons sit on top as the click targets.
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import MinusIcon from '@lucide/svelte/icons/minus';
	import SquareIcon from '@lucide/svelte/icons/square';
	import XIcon from '@lucide/svelte/icons/x';

	const appWindow = getCurrentWindow();
</script>

<header data-tauri-drag-region class="titlebar">
	<div class="flex items-center gap-2.5">
		<img src="/brand/logo.svg" alt="ozky" class="size-15 rounded-[5px]" />
		<!-- <span class="font-heading text-sm font-semibold tracking-tight">ozky</span>
		<Badge variant="outline" class="h-5 gap-1.5 px-2 text-[10px] font-medium uppercase tracking-wider">
			<span class="size-1.5 rounded-full bg-primary"></span>
			{wallet.network}
		</Badge> -->
	</div>

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
</header>

<style>
	.titlebar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		height: 38px;
		padding-left: 14px;
		padding-right: 6px;
		border-bottom: 1px solid var(--border);
		background: color-mix(in oklch, var(--background) 80%, transparent);
		backdrop-filter: blur(12px);
		user-select: none;
	}
	.controls {
		display: flex;
		gap: 2px;
	}
	.ctl {
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
