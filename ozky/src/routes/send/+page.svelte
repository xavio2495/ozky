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
	import { toBaseUnits, assetByCode } from '$lib/assets';
	import { truncate } from '$lib/format';

	// Default cross-asset slippage tolerance (1%) — the sender over-spends X by this much so the
	// in-pool quote still covers the recipient's amount if reserves move before submit.
	const PAY_SLIPPAGE_BPS = 100;

	let asset = $state('USDC');
	let recvAsset = $state('USDC');
	let recipient = $state('');
	let amount = $state('');
	let payCost = $state<number | null>(null);
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
	// The amount is always denominated in the RECEIVE asset (= pay asset for a normal send).
	const recvDecimals = $derived(
		wallet.balances.find((b) => b.code === recvAsset)?.decimals ??
			assetByCode(recvAsset)?.decimals ??
			7
	);
	const isCrossAsset = $derived(recvAsset !== asset);
	const payDecimals = $derived(bal?.decimals ?? assetByCode(asset)?.decimals ?? 7);
	const payCostDisplay = $derived(
		payCost === null ? null : (payCost / 10 ** payDecimals).toLocaleString('en-US', { maximumFractionDigits: payDecimals })
	);

	// Live reverse-quote: the X the sender spends to deliver `amount` of the receive asset.
	$effect(() => {
		const a = amount;
		const from = asset;
		const to = recvAsset;
		if (!isCrossAsset || !a) {
			payCost = null;
			return;
		}
		let cancelled = false;
		(async () => {
			try {
				const units = toBaseUnits(a, recvDecimals);
				const q = await api.payQuote(from, to, units);
				if (!cancelled) payCost = q.source_cost;
			} catch {
				if (!cancelled) payCost = null;
			}
		})();
		return () => {
			cancelled = true;
		};
	});

	function review() {
		if (!recipient.trim().startsWith('ozky')) {
			toast.error('Enter a valid ozky… recipient code');
			return;
		}
		try {
			toBaseUnits(amount, recvDecimals);
		} catch (e) {
			toast.error(errMessage(e));
			return;
		}
		confirmOpen = true;
	}

	async function submit() {
		confirmOpen = false;
		const units = toBaseUnits(amount, recvDecimals);
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
		const dest = recipient.trim();
		const hash = await runAction(
			'Sending payment',
			() =>
				isCrossAsset
					? api.pay(dest, asset, recvAsset, units, PAY_SLIPPAGE_BPS).then((r) => r.tx_hash)
					: api.send(asset, dest, units),
			{ success: () => 'Payment sent' }
		);
		proving = false;
		if (hash) {
			const label = isCrossAsset
				? `Paid ${amount} ${recvAsset} (in ${asset})`
				: `Sent ${amount} ${asset}`;
			wallet.log({ kind: 'send', label, detail: truncate(recipient), hash });
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
						<Field.Label>Recipient receives</Field.Label>
						<AssetSelect bind:value={recvAsset} />
						<Field.Description>
							{#if isCrossAsset}
								You pay in {asset}; they receive {recvAsset}, converted in-pool at the live rate.
							{:else}
								Same asset you pay in. Pick a different one to pay across assets.
							{/if}
						</Field.Description>
					</Field.Field>
					<Field.Field>
						<Field.Label>{isCrossAsset ? `Amount (${recvAsset} they receive)` : 'Amount'}</Field.Label>
						<AmountInput
							bind:value={amount}
							code={recvAsset}
							decimals={recvDecimals}
							max={isCrossAsset ? undefined : bal?.raw}
						/>
						{#if isCrossAsset}
							<Field.Description>
								{#if payCostDisplay}≈ {payCostDisplay} {asset} to send · {/if}Available: {bal?.display ?? '0'} {asset}
							</Field.Description>
						{:else if bal}
							<Field.Description>Available: {bal.display} {asset}</Field.Description>
						{/if}
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
				{#if isCrossAsset}
					Pay <span class="font-mono">{truncate(recipient)}</span> so they receive
					<b>{amount} {recvAsset}</b>{#if payCostDisplay}, costing about <b>{payCostDisplay} {asset}</b>{/if}.
					The amount is public on-chain (priced by the in-pool AMM); the recipient stays hidden.
				{:else}
					Send <b>{amount} {asset}</b> to <span class="font-mono">{truncate(recipient)}</span>?
				{/if}
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
