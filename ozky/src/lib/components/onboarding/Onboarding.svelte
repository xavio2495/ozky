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
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CircleIcon from '@lucide/svelte/icons/circle';
	import { api, errMessage, type WalletSetup } from '$lib/api';
	import { wallet } from '$lib/wallet.svelte';

	type Step =
		| 'welcome'
		| 'create-password'
		| 'reveal'
		| 'restore-phrase'
		| 'restore-password'
		| 'twofa-qr'
		| 'twofa-verify';
	let step = $state<Step>('welcome');
	let busy = $state(false);

	let password = $state('');
	let confirm = $state('');
	let restoreText = $state('');
	let setup = $state<WalletSetup | null>(null);
	let otp = $state('');

	const words = $derived(setup?.mnemonic ? setup.mnemonic.trim().split(/\s+/) : []);
	const restoreWordCount = $derived(restoreText.trim().split(/\s+/).filter(Boolean).length);

	// Strict password policy: length + character-class diversity.
	const pwChecks = $derived({
		length: password.length >= 10,
		upper: /[A-Z]/.test(password),
		lower: /[a-z]/.test(password),
		number: /[0-9]/.test(password),
		symbol: /[^A-Za-z0-9]/.test(password)
	});
	const pwStrong = $derived(Object.values(pwChecks).every(Boolean));
	const passwordOk = $derived(pwStrong && confirm.length > 0 && password === confirm);

	function resetCreds() {
		password = '';
		confirm = '';
	}

	async function create() {
		busy = true;
		try {
			setup = await api.createWallet(password);
			resetCreds();
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
			resetCreds();
			step = 'twofa-qr';
		} catch (e) {
			toast.error('Could not restore wallet', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}

	async function confirm2fa() {
		busy = true;
		try {
			// Commits the staged wallet only once the 2FA code checks out — before this the
			// vault isn't written, so a reload/abandon can't leave a half-made, locked-out wallet.
			const ok = await api.finishSetup(otp);
			if (!ok) {
				toast.error('That code is incorrect', { description: 'Check your authenticator app.' });
				otp = '';
				return;
			}
			await wallet.refreshStatus();
			await wallet.loadSession();
			toast.success('Wallet ready');
			// Best-effort: ask the server funder to create + fund the account (10 XLM), then
			// add the USDC/EURC trustlines locally. Non-fatal — dev without a funder configured
			// just skips it and the user can retry from Settings.
			void provisionAccount();
		} catch (e) {
			toast.error('Verification failed', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}

	async function provisionAccount() {
		try {
			const report = await api.provisionAccount();
			if (report.account_created || report.added.length) {
				toast.success('Account funded & trustlines ready', {
					description: report.added.length ? `Trustlines: ${report.added.join(', ')}` : undefined
				});
				await wallet.refreshPublicBalances();
			}
		} catch {
			// Onboarding nicety; silently skip when the funder isn't configured.
		}
	}
</script>

{#snippet backLink(target: Step)}
	<button type="button" class="back-link" onclick={() => (step = target)}>
		<ArrowLeftIcon class="size-3.5" />
		Back
	</button>
{/snippet}

{#snippet passwordFields()}
	<Field.Field>
		<Field.Label for="pw">Password</Field.Label>
		<Input id="pw" type="password" bind:value={password} placeholder="Create a strong password" />
	</Field.Field>
	<ul class="pw-checks">
		<li class:ok={pwChecks.length}>
			{#if pwChecks.length}<CheckIcon class="size-3.5" />{:else}<CircleIcon class="size-3.5" />{/if}
			10+ characters
		</li>
		<li class:ok={pwChecks.upper}>
			{#if pwChecks.upper}<CheckIcon class="size-3.5" />{:else}<CircleIcon class="size-3.5" />{/if}
			Uppercase
		</li>
		<li class:ok={pwChecks.lower}>
			{#if pwChecks.lower}<CheckIcon class="size-3.5" />{:else}<CircleIcon class="size-3.5" />{/if}
			Lowercase
		</li>
		<li class:ok={pwChecks.number}>
			{#if pwChecks.number}<CheckIcon class="size-3.5" />{:else}<CircleIcon class="size-3.5" />{/if}
			Number
		</li>
		<li class:ok={pwChecks.symbol}>
			{#if pwChecks.symbol}<CheckIcon class="size-3.5" />{:else}<CircleIcon class="size-3.5" />{/if}
			Symbol
		</li>
	</ul>
	<Field.Field>
		<Field.Label for="pw2">Confirm password</Field.Label>
		<Input id="pw2" type="password" bind:value={confirm} />
		{#if confirm && password !== confirm}
			<Field.Description class="text-destructive">Passwords don't match.</Field.Description>
		{/if}
	</Field.Field>
{/snippet}

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
							<Button variant="outline" onclick={() => (step = 'restore-phrase')}>
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
						{@render backLink('welcome')}
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">Set a password</h1>
							<p class="text-sm text-muted-foreground">
								This password encrypts your wallet on this device. There is no reset — if you
								forget it, restore from your recovery phrase.
							</p>
						</div>
						{@render passwordFields()}
						<Field.Field>
							<Button onclick={create} disabled={busy || !passwordOk}>
								{#if busy}<Spinner data-icon="inline-start" />{/if}
								Create wallet
							</Button>
						</Field.Field>
					</Field.Group>
				{:else if step === 'reveal'}
					<Field.Group class="gap-4">
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
							<Button onclick={() => (step = 'twofa-qr')}>
								I've saved it — set up 2FA
								<ArrowRightIcon data-icon="inline-end" />
							</Button>
						</Field.Field>
					</Field.Group>
				{:else if step === 'restore-phrase'}
					<Field.Group>
						{@render backLink('welcome')}
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">Restore wallet</h1>
							<p class="text-sm text-muted-foreground">
								Enter your 12-word recovery phrase to restore your wallet on this device.
							</p>
						</div>
						<Field.Field>
							<Field.Label for="phrase">Recovery phrase</Field.Label>
							<Textarea
								id="phrase"
								bind:value={restoreText}
								rows={3}
								placeholder="word1 word2 word3 …"
								class="font-mono"
							/>
							<Field.Description>{restoreWordCount}/12 words</Field.Description>
						</Field.Field>
						<Field.Field>
							<Button
								onclick={() => (step = 'restore-password')}
								disabled={restoreWordCount !== 12}
							>
								Continue
								<ArrowRightIcon data-icon="inline-end" />
							</Button>
						</Field.Field>
					</Field.Group>
				{:else if step === 'restore-password'}
					<Field.Group>
						{@render backLink('restore-phrase')}
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">Set a password</h1>
							<p class="text-sm text-muted-foreground">
								Choose a new password to encrypt your restored wallet on this device.
							</p>
						</div>
						{@render passwordFields()}
						<Field.Field>
							<Button onclick={restore} disabled={busy || !passwordOk}>
								{#if busy}<Spinner data-icon="inline-start" />{/if}
								Restore wallet
							</Button>
						</Field.Field>
					</Field.Group>
				{:else if step === 'twofa-qr'}
					<Field.Group class="gap-4">
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">Set up 2FA</h1>
							<p class="text-sm text-muted-foreground">
								Scan this with an authenticator app (Google Authenticator, Authy…), or add the
								key manually.
							</p>
						</div>
						{#if setup}
							<!-- Plain black-on-white QR: some authenticator apps fail to scan coloured codes. -->
							<div class="flex justify-center"><Qr data={setup.totp_uri} /></div>
							<div class="flex items-center justify-between gap-3">
								<span class="text-xs text-muted-foreground">Or enter the key manually:</span>
								<CopyButton text={`${setup.totp_uri}\n${setup.totp_secret}`} label="Copy key & QR link" />
							</div>
							<p
								class="break-all rounded-md border border-border bg-muted/50 p-2 text-center font-mono text-xs"
							>
								{setup.totp_secret}
							</p>
						{/if}
						<Field.Field>
							<Button onclick={() => (step = 'twofa-verify')}>
								I've added it — continue
								<ArrowRightIcon data-icon="inline-end" />
							</Button>
						</Field.Field>
					</Field.Group>
				{:else if step === 'twofa-verify'}
					<Field.Group>
						{@render backLink('twofa-qr')}
						<div class="flex flex-col gap-2">
							<h1 class="font-heading text-2xl font-semibold tracking-tight">Confirm 2FA</h1>
							<p class="text-sm text-muted-foreground">
								Enter the 6-digit code from your authenticator app to finish setup.
							</p>
						</div>
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
	.back-link {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		align-self: flex-start;
		font-size: 0.8125rem;
		color: var(--muted-foreground);
		transition: color 0.15s ease;
		cursor: pointer;
	}
	.back-link:hover {
		color: var(--foreground);
	}
	.pw-checks {
		display: grid;
		grid-template-columns: repeat(2, 1fr);
		gap: 4px 12px;
	}
	.pw-checks li {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 0.75rem;
		color: var(--muted-foreground);
		transition: color 0.15s ease;
	}
	.pw-checks li.ok {
		color: var(--primary);
	}
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
