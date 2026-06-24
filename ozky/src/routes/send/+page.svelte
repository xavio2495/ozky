<script lang="ts">
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import AssetSelect from '$lib/components/shared/AssetSelect.svelte';
	import AmountInput from '$lib/components/shared/AmountInput.svelte';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import ShieldIcon from '@lucide/svelte/icons/shield';
	import { toast } from 'svelte-sonner';
	import { api, errMessage } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import { settings, type PrivacyMode } from '$lib/settings.svelte';
	import { toBaseUnits } from '$lib/assets';
	import { truncate } from '$lib/format';

	let asset = $state('USDC');
	let recipient = $state('');
	let amount = $state('');
	let confirmOpen = $state(false);
	let proving = $state(false);
	// Per-send privacy mode, defaulting to the saved preference. Client-side strategy only.
	let mode = $state<PrivacyMode>(settings.privacyMode);

	// Max-privacy timing jitter: a randomized client-side delay before submit, decorrelating the
	// on-chain submission time from the click. "Send now" skips the wait; "Cancel" aborts.
	let delaying = $state(false);
	let delayRemaining = $state(0);
	let resolveDelay: ((proceed: boolean) => void) | null = null;
	let delayTimer: ReturnType<typeof setInterval> | null = null;

	const fmtCountdown = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;

	function waitPrivacyDelay(ms: number): Promise<boolean> {
		return new Promise((resolve) => {
			delaying = true;
			delayRemaining = Math.ceil(ms / 1000);
			const started = Date.now();
			resolveDelay = (proceed: boolean) => {
				if (delayTimer) clearInterval(delayTimer);
				delayTimer = null;
				delaying = false;
				resolveDelay = null;
				resolve(proceed);
			};
			delayTimer = setInterval(() => {
				const left = ms - (Date.now() - started);
				delayRemaining = Math.max(0, Math.ceil(left / 1000));
				if (left <= 0) resolveDelay?.(true);
			}, 250);
		});
	}

	const bal = $derived(wallet.balances.find((b) => b.code === asset));

	function review() {
		if (!recipient.trim().startsWith('ozky')) {
			toast.error('Enter a valid ozky… recipient code');
			return;
		}
		try {
			toBaseUnits(amount, bal?.decimals ?? 7);
		} catch (e) {
			toast.error(errMessage(e));
			return;
		}
		confirmOpen = true;
	}

	async function submit() {
		confirmOpen = false;
		const units = toBaseUnits(amount, bal?.decimals ?? 7);
		// Maximum-privacy mode: hold the submission for a randomized client-side delay first.
		const delayMs = settings.privacyDelayMs(mode);
		if (delayMs > 0) {
			const proceed = await waitPrivacyDelay(delayMs);
			if (!proceed) {
				toast.message('Payment cancelled');
				return;
			}
		}
		proving = true;
		const hash = await runAction('Sending payment', () => api.send(asset, recipient.trim(), units), {
			success: () => 'Payment sent'
		});
		proving = false;
		if (hash) {
			wallet.log({ kind: 'send', label: `Sent ${amount} ${asset}`, detail: truncate(recipient), hash });
			amount = '';
			recipient = '';
		}
	}
</script>

<Workspace title="Send" subtitle="Send a private, shielded payment to another ozky wallet">
	{#snippet main()}
		<Card.Root class="max-w-xl">
			<Card.Header>
				<Card.Title>New payment</Card.Title>
				<Card.Description>Amount and parties stay hidden on-chain.</Card.Description>
			</Card.Header>
			<Card.Content>
				<Field.Group>
					<Field.Field>
						<Field.Label>Asset</Field.Label>
						<AssetSelect bind:value={asset} />
					</Field.Field>
					<Field.Field>
						<Field.Label for="recipient">Recipient</Field.Label>
						<Input id="recipient" bind:value={recipient} placeholder="ozky…" class="font-mono" />
						<Field.Description>The recipient's shielded payment code.</Field.Description>
					</Field.Field>
					<Field.Field>
						<Field.Label>Amount</Field.Label>
						<AmountInput bind:value={amount} code={asset} decimals={bal?.decimals ?? 7} max={bal?.raw} />
						{#if bal}<Field.Description>Available: {bal.display} {asset}</Field.Description>{/if}
					</Field.Field>
					<Field.Field>
						<Field.Label>Privacy</Field.Label>
						<div class="grid grid-cols-2 gap-2">
							<button type="button" class="mode" data-active={mode === 'instant'} onclick={() => (mode = 'instant')}>
								<ZapIcon class="size-4" />
								<span class="flex flex-col items-start">
									<span class="text-sm font-medium">Instant</span>
									<span class="text-xs text-muted-foreground">Submit right away</span>
								</span>
							</button>
							<button type="button" class="mode" data-active={mode === 'max'} onclick={() => (mode = 'max')}>
								<ShieldIcon class="size-4" />
								<span class="flex flex-col items-start">
									<span class="text-sm font-medium">Maximum privacy</span>
									<span class="text-xs text-muted-foreground">May take a few minutes</span>
								</span>
							</button>
						</div>
						<Field.Description>
							A timing strategy on this device — both look identical on-chain.
						</Field.Description>
					</Field.Field>
				</Field.Group>
			</Card.Content>
			<Card.Footer>
				<Button onclick={review} disabled={!recipient || !amount}>Review payment</Button>
			</Card.Footer>
		</Card.Root>
	{/snippet}

	{#snippet aside()}
		<Alert.Root>
			<EyeOffIcon />
			<Alert.Title>Fully shielded</Alert.Title>
			<Alert.Description>
				<ul class="mt-1 flex list-disc flex-col gap-1 pl-4 text-xs">
					<li>Amount, sender, and receiver are hidden on-chain.</li>
					<li>The network fee is paid by a relayer — your wallet's XLM is untouched.</li>
					<li>A zero-knowledge proof is generated locally before submitting.</li>
				</ul>
			</Alert.Description>
		</Alert.Root>
	{/snippet}
</Workspace>

<AlertDialog.Root bind:open={confirmOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Confirm payment</AlertDialog.Title>
			<AlertDialog.Description>
				Send <b>{amount} {asset}</b> to <span class="font-mono">{truncate(recipient)}</span>?
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={submit}>Send payment</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<!-- Maximum-privacy timing delay -->
<AlertDialog.Root open={delaying}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Maximizing privacy…</AlertDialog.Title>
			<AlertDialog.Description>
				Holding your payment for <b>{fmtCountdown(delayRemaining)}</b> so its submission time
				reveals nothing. Keep ozky open. You can send immediately, or cancel.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<Button variant="outline" onclick={() => resolveDelay?.(false)}>Cancel</Button>
			<Button onclick={() => resolveDelay?.(true)}>Send now</Button>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title="Sending payment" />

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
