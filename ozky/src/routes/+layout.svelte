<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import Titlebar from '$lib/components/chrome/Titlebar.svelte';
	import TopNav from '$lib/components/nav/TopNav.svelte';
	import Onboarding from '$lib/components/onboarding/Onboarding.svelte';
	import SignIn from '$lib/components/onboarding/SignIn.svelte';
	import RuneGlyphField from '$lib/components/chrome/RuneGlyphField.svelte';
	import { Toaster } from '$lib/components/ui/sonner';
	import { Spinner } from '$lib/components/ui/spinner';
	import { wallet } from '$lib/wallet.svelte';
	import { installDevLog } from '$lib/devlog';

	const { children } = $props();

	let ready = $state(false);
	onMount(async () => {
		installDevLog();
		try {
			await wallet.refreshStatus();
			if (wallet.unlocked) await wallet.loadSession();
		} catch {
			/* surfaced on first interaction */
		} finally {
			ready = true;
		}
	});
</script>

<Toaster theme="dark" position="top-center" richColors />

<div class="app">
	<Titlebar />
	<div class="below">
		{#if !ready}
			<div class="grid h-full w-full place-items-center">
				<Spinner class="size-6 text-muted-foreground" />
			</div>
		{:else if !wallet.unlocked}
			<div class="auth-screen">
				<RuneGlyphField />
				<div class="auth-content">
					{#if !wallet.initialized}
						<Onboarding />
					{:else}
						<SignIn />
					{/if}
				</div>
			</div>
		{:else}
			<div class="body">
				<RuneGlyphField />
				<div class="shell">
					<TopNav />
					<main class="content">
						{@render children()}
					</main>
				</div>
			</div>
		{/if}
	</div>
</div>

<style>
	.app {
		display: flex;
		flex-direction: column;
		height: 100vh;
		overflow: hidden;
	}
	.below {
		display: flex;
		flex: 1;
		min-height: 0;
	}
	.body {
		position: relative;
		flex: 1;
		min-height: 0;
		width: 100%;
		overflow: hidden;
	}
	/* The rune field is an absolute z-0 backdrop; the shell floats above it. */
	.shell {
		position: relative;
		z-index: 1;
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.content {
		flex: 1;
		min-width: 0;
		min-height: 0;
		overflow: hidden;
	}
	.auth-screen {
		position: relative;
		flex: 1;
		min-height: 0;
		width: 100%;
		overflow: hidden;
	}
	.auth-content {
		position: relative;
		z-index: 1;
		height: 100%;
		width: 100%;
	}
</style>
