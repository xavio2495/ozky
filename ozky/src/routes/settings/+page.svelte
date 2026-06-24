<script lang="ts">
	import { onMount } from 'svelte';
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import AddressField from '$lib/components/shared/AddressField.svelte';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import { Badge } from '$lib/components/ui/badge';
	import * as Card from '$lib/components/ui/card';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Alert from '$lib/components/ui/alert';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import ShieldIcon from '@lucide/svelte/icons/shield';
	import { toast } from 'svelte-sonner';
	import { api, errMessage } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import { settings } from '$lib/settings.svelte';

	let funding = $state('');
	let keyOpen = $state(false);
	let spendingKey = $state('');
	let loadingKey = $state(false);
	let enrolling = $state(false);

	onMount(async () => {
		try {
			funding = await api.fundingAddress();
		} catch (e) {
			toast.error('Could not load address', { description: errMessage(e) });
		}
	});

	async function revealKey() {
		keyOpen = true;
		if (spendingKey) return;
		loadingKey = true;
		try {
			spendingKey = await api.spendingKey();
		} catch (e) {
			toast.error('Could not load key', { description: errMessage(e) });
		} finally {
			loadingKey = false;
		}
	}

	async function enroll() {
		enrolling = true;
		const hash = await runAction('Enrolling into ASP', () => api.enroll(), {
			success: () => 'Enrolled',
			refresh: false
		});
		enrolling = false;
		if (hash) wallet.log({ kind: 'enroll', label: 'Enrolled into ASP', hash });
	}
</script>

<Workspace title="Settings" subtitle="Wallet identity, keys, and compliance">
	{#snippet main()}
		<div class="flex max-w-xl flex-col gap-5">
			<Card.Root>
				<Card.Header>
					<Card.Title>Identity</Card.Title>
					<Card.Description>This wallet's public Stellar account.</Card.Description>
				</Card.Header>
				<Card.Content>
					<AddressField label="Funding address" value={funding} loading={!funding} />
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header>
					<Card.Title>Compliance</Card.Title>
					<Card.Description>Join the pool's approved set to send shielded payments.</Card.Description>
				</Card.Header>
				<Card.Content class="flex items-center justify-between gap-3">
					<p class="text-sm text-muted-foreground">ASP enrollment (testnet admin only).</p>
					<Button variant="outline" onclick={enroll} disabled={enrolling}>
						{#if enrolling}<Spinner data-icon="inline-start" />{:else}<ShieldCheckIcon
								data-icon="inline-start"
							/>{/if}
						Enroll
					</Button>
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header>
					<Card.Title>Default privacy</Card.Title>
					<Card.Description>
						The default mode for new payments. A timing strategy on this device only — both
						modes look identical on-chain.
					</Card.Description>
				</Card.Header>
				<Card.Content>
					<div class="grid grid-cols-2 gap-2">
						<button type="button" class="mode" data-active={settings.privacyMode === 'instant'} onclick={() => (settings.privacyMode = 'instant')}>
							<ZapIcon class="size-4" />
							<span class="flex flex-col items-start">
								<span class="text-sm font-medium">Instant</span>
								<span class="text-xs text-muted-foreground">Submit right away</span>
							</span>
						</button>
						<button type="button" class="mode" data-active={settings.privacyMode === 'max'} onclick={() => (settings.privacyMode = 'max')}>
							<ShieldIcon class="size-4" />
							<span class="flex flex-col items-start">
								<span class="text-sm font-medium">Maximum privacy</span>
								<span class="text-xs text-muted-foreground">Delay before submit</span>
							</span>
						</button>
					</div>
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header>
					<Card.Title>Keys</Card.Title>
					<Card.Description>Your spending public key (shareable) and network.</Card.Description>
				</Card.Header>
				<Card.Content class="flex items-center justify-between gap-3">
					<div class="flex items-center gap-2 text-sm">
						Network <Badge variant="outline" class="uppercase">{wallet.network}</Badge>
					</div>
					<Button variant="outline" onclick={revealKey}>
						<KeyRoundIcon data-icon="inline-start" />
						Reveal spending key
					</Button>
				</Card.Content>
			</Card.Root>
		</div>
	{/snippet}
</Workspace>

<Dialog.Root bind:open={keyOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Spending public key</Dialog.Title>
			<Dialog.Description>Share this with an ASP operator to be enrolled. It is public — not a secret seed.</Dialog.Description>
		</Dialog.Header>
		<div class="flex flex-col gap-3">
			{#if loadingKey}
				<div class="grid place-items-center py-6"><Spinner /></div>
			{:else}
				<p class="rounded-lg border border-border bg-muted/50 p-3 font-mono text-xs break-all">{spendingKey}</p>
				<div class="flex justify-end"><CopyButton text={spendingKey} label="Copy key" /></div>
			{/if}
			<Alert.Root variant="destructive">
				<TriangleAlertIcon />
				<Alert.Description>
					Never share your 12-word recovery phrase. Only this public key is safe to share.
				</Alert.Description>
			</Alert.Root>
		</div>
	</Dialog.Content>
</Dialog.Root>

<style>
	.mode {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		text-align: left;
		transition: border-color 0.15s ease, background 0.15s ease;
	}
	.mode:hover {
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
	.mode[data-active='true'] {
		border-color: var(--primary);
		background: color-mix(in oklch, var(--primary) 8%, transparent);
		color: var(--primary);
	}
</style>
