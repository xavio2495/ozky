<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import Titlebar from '$lib/components/chrome/Titlebar.svelte';
	import Sidebar from '$lib/components/nav/Sidebar.svelte';
	import Onboarding from '$lib/components/onboarding/Onboarding.svelte';
	import SignIn from '$lib/components/onboarding/SignIn.svelte';
	import RuneGlyphField from '$lib/components/chrome/RuneGlyphField.svelte';
	import { Toaster } from '$lib/components/ui/sonner';
	import { Spinner } from '$lib/components/ui/spinner';
	import { wallet } from '$lib/wallet.svelte';

	const { children } = $props();

	let ready = $state(false);
	onMount(async () => {
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

<Toaster theme="dark" position="bottom-right" richColors />

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
				<Sidebar />
				<main class="content">
					{@render children()}
				</main>
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
		display: flex;
		flex: 1;
		min-height: 0;
		width: 100%;
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
