<script lang="ts">
	import { fly } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import { toast } from 'svelte-sonner';
	import OnboardShell from './OnboardShell.svelte';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import Qr from '$lib/components/shared/Qr.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import * as InputOTP from '$lib/components/ui/input-otp';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import { api, errMessage, type WalletSetup } from '$lib/api';
	import { wallet } from '$lib/wallet.svelte';

	type Step = 'welcome' | 'create-password' | 'reveal' | 'restore' | 'twofa';
	let step = $state<Step>('welcome');
	let busy = $state(false);

	let password = $state('');
	let confirm = $state('');
	let restoreText = $state('');
	let setup = $state<WalletSetup | null>(null);
	let otp = $state('');

	const words = $derived(setup?.mnemonic ? setup.mnemonic.trim().split(/\s+/) : []);
	const restoreWordCount = $derived(restoreText.trim().split(/\s+/).filter(Boolean).length);
	const passwordOk = $derived(password.length >= 8 && password === confirm);

	async function create() {
		busy = true;
		try {
			setup = await api.createWallet(password);
			password = '';
			confirm = '';
			step = 'reveal';
		} catch (e) {
			toast.error('Could not create wallet', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}

	async function restore() {
		busy = true;
		try {
			setup = await api.restoreWallet(restoreText.trim(), password);
			restoreText = '';
			password = '';
			confirm = '';
			step = 'twofa';
		} catch (e) {
			toast.error('Could not restore wallet', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}

	async function confirm2fa() {
		busy = true;
		try {
			const ok = await api.verifyTotp(otp);
			if (!ok) {
				toast.error('That code is incorrect', { description: 'Check your authenticator app.' });
				otp = '';
				return;
			}
			// Vault is created and the session is already unlocked — enter the app.
			await wallet.refreshStatus();
			await wallet.loadSession();
			toast.success('Wallet ready');
		} catch (e) {
			toast.error('Verification failed', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}
</script>

<div class="grid h-full w-full place-items-center p-6">
	{#key step}
		<div in:fly={{ y: 14, duration: 300, easing: cubicOut }}>
			<OnboardShell>
				{#if step === 'welcome'}
					<Field.Group>
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">Welcome to ozky</h1>
							<p class="text-sm text-balance text-muted-foreground">
								Create a new shielded wallet, or restore one from your recovery phrase.
							</p>
						</div>
						<Field.Field>
							<Button onclick={() => (step = 'create-password')}>
								<PlusIcon data-icon="inline-start" />
								Create a new wallet
							</Button>
						</Field.Field>
						<Field.Separator>or</Field.Separator>
						<Field.Field>
							<Button variant="outline" onclick={() => (step = 'restore')}>
								<RotateCcwIcon data-icon="inline-start" />
								Restore from recovery phrase
							</Button>
						</Field.Field>
						<Field.Description class="text-center">
							Your seed is encrypted on this device and unlocked with a password + 2FA.
						</Field.Description>
					</Field.Group>
				{:else if step === 'create-password'}
					<Field.Group>
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">Set a password</h1>
							<p class="text-sm text-muted-foreground">
								This password encrypts your wallet on this device. There is no reset — if you
								forget it, restore from your recovery phrase.
							</p>
						</div>
						<Field.Field>
							<Field.Label for="pw">Password</Field.Label>
							<Input id="pw" type="password" bind:value={password} placeholder="At least 8 characters" />
						</Field.Field>
						<Field.Field>
							<Field.Label for="pw2">Confirm password</Field.Label>
							<Input id="pw2" type="password" bind:value={confirm} />
							{#if confirm && password !== confirm}
								<Field.Description class="text-destructive">Passwords don't match.</Field.Description>
							{/if}
						</Field.Field>
						<Field.Field>
							<Button onclick={create} disabled={busy || !passwordOk}>
								{#if busy}<Spinner data-icon="inline-start" />{/if}
								Create wallet
							</Button>
							<Button variant="ghost" onclick={() => (step = 'welcome')}>
								<ArrowLeftIcon data-icon="inline-start" />
								Back
							</Button>
						</Field.Field>
					</Field.Group>
				{:else if step === 'reveal'}
					<Field.Group>
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">
								Your recovery phrase
							</h1>
							<p class="text-sm text-muted-foreground">
								Write these 12 words down in order and keep them somewhere safe.
							</p>
						</div>
						<ol class="phrase">
							{#each words as word, i}
								<li><span class="num">{i + 1}</span>{word}</li>
							{/each}
						</ol>
						<div class="flex justify-end">
							<CopyButton text={words.join(' ')} label="Copy phrase" />
						</div>
						<Alert.Root variant="destructive">
							<TriangleAlertIcon />
							<Alert.Title>Shown only once</Alert.Title>
							<Alert.Description>
								Anyone with this phrase controls your funds. ozky cannot recover it for you.
							</Alert.Description>
						</Alert.Root>
						<Field.Field>
							<Button onclick={() => (step = 'twofa')}>I've saved it — set up 2FA</Button>
						</Field.Field>
					</Field.Group>
				{:else if step === 'restore'}
					<Field.Group>
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">Restore wallet</h1>
							<p class="text-sm text-muted-foreground">
								Enter your 12-word recovery phrase and set a new password for this device.
							</p>
						</div>
						<Field.Field>
							<Field.Label for="phrase">Recovery phrase</Field.Label>
							<Textarea id="phrase" bind:value={restoreText} rows={3} placeholder="word1 word2 word3 …" class="font-mono" />
							<Field.Description>{restoreWordCount}/12 words</Field.Description>
						</Field.Field>
						<Field.Field>
							<Field.Label for="rpw">New password</Field.Label>
							<Input id="rpw" type="password" bind:value={password} placeholder="At least 8 characters" />
						</Field.Field>
						<Field.Field>
							<Field.Label for="rpw2">Confirm password</Field.Label>
							<Input id="rpw2" type="password" bind:value={confirm} />
						</Field.Field>
						<Field.Field>
							<Button onclick={restore} disabled={busy || restoreWordCount !== 12 || !passwordOk}>
								{#if busy}<Spinner data-icon="inline-start" />{/if}
								Restore wallet
							</Button>
							<Button variant="ghost" onclick={() => (step = 'welcome')}>
								<ArrowLeftIcon data-icon="inline-start" />
								Back
							</Button>
						</Field.Field>
					</Field.Group>
				{:else if step === 'twofa'}
					<Field.Group>
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">Set up 2FA</h1>
							<p class="text-sm text-muted-foreground">
								Scan this with an authenticator app (Google Authenticator, Authy…), then enter
								the 6-digit code to confirm.
							</p>
						</div>
						{#if setup}
							<div class="flex justify-center"><Qr data={setup.totp_uri} /></div>
							<div class="flex items-center justify-between gap-3">
								<span class="text-xs text-muted-foreground">Or enter the key manually:</span>
								<CopyButton text={setup.totp_secret} label="Copy key" />
							</div>
							<p class="break-all rounded-md border border-border bg-muted/50 p-2 text-center font-mono text-xs">
								{setup.totp_secret}
							</p>
						{/if}
						<Field.Field class="items-center">
							<InputOTP.Root maxlength={6} bind:value={otp} class="gap-3">
								{#snippet children({ cells })}
									<InputOTP.Group>
										{#each cells.slice(0, 3) as cell (cell)}
											<InputOTP.Slot {cell} />
										{/each}
									</InputOTP.Group>
									<InputOTP.Separator />
									<InputOTP.Group>
										{#each cells.slice(3, 6) as cell (cell)}
											<InputOTP.Slot {cell} />
										{/each}
									</InputOTP.Group>
								{/snippet}
							</InputOTP.Root>
						</Field.Field>
						<Field.Field>
							<Button onclick={confirm2fa} disabled={busy || otp.length !== 6}>
								{#if busy}<Spinner data-icon="inline-start" />{:else}<ShieldCheckIcon
										data-icon="inline-start"
									/>{/if}
								Confirm & finish
							</Button>
						</Field.Field>
					</Field.Group>
				{/if}
			</OnboardShell>
		</div>
	{/key}
</div>

<style>
	.phrase {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 8px;
	}
	.phrase li {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		background: color-mix(in oklch, var(--muted) 50%, transparent);
		font-family: var(--font-mono, monospace);
		font-size: 0.8125rem;
	}
	.phrase .num {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
		min-width: 14px;
	}
</style>
