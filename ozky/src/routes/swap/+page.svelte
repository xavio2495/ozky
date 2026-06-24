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
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import { toast } from 'svelte-sonner';
	import { api, errMessage, type SwapQuote } from '$lib/api';
	import { wallet } from '$lib/wallet.svelte';
	import { toBaseUnits, assetByCode } from '$lib/assets';

	let from = $state('USDC');
	let to = $state('EURC');
	let amount = $state('');
	let slippagePct = $state('1');
	let quote = $state<SwapQuote | null>(null);
	let quoting = $state(false);
	let quoteErr = $state('');
	let confirmOpen = $state(false);
	let proving = $state(false);

	const bal = $derived(wallet.balances.find((b) => b.code === from));
	const toDecimals = $derived(assetByCode(to)?.decimals ?? 7);
	const slippageBps = $derived(Math.round((Number(slippagePct) || 0) * 100));
	const minReceived = $derived(
		quote ? Math.floor((quote.dest_amount * (10_000 - slippageBps)) / 10_000) : 0
	);

	/** Base units -> display number for the destination asset. */
	const fmt = (units: number, decimals = toDecimals) =>
		(units / 10 ** decimals).toLocaleString('en-US', { maximumFractionDigits: decimals });

	// Auto-quote (debounced) whenever the pair / amount changes and are valid.
	let quoteTimer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => {
		const f = from,
			t = to,
			a = amount;
		quote = null;
		quoteErr = '';
		clearTimeout(quoteTimer);
		if (f === t || !a.trim()) return;
		let units: number;
		try {
			units = toBaseUnits(a, bal?.decimals ?? 7);
		} catch {
			return;
		}
		quoting = true;
		quoteTimer = setTimeout(async () => {
			try {
				quote = await api.swapQuote(f, t, units);
			} catch (e) {
				quoteErr = errMessage(e);
			} finally {
				quoting = false;
			}
		}, 450);
	});

	function review() {
		if (from === to) {
			toast.error('Choose two different assets');
			return;
		}
		try {
			toBaseUnits(amount, bal?.decimals ?? 7);
		} catch (e) {
			toast.error(errMessage(e));
			return;
		}
		if (slippageBps <= 0 || slippageBps > 5000) {
			toast.error('Slippage must be between 0% and 50%');
			return;
		}
		confirmOpen = true;
	}

	async function submit() {
		confirmOpen = false;
		const units = toBaseUnits(amount, bal?.decimals ?? 7);
		proving = true;
		try {
			const r = await api.swap(from, to, units, slippageBps);
			toast.success(`Swapped ${amount} ${from} → ${fmt(r.received)} ${to}`);
			wallet.log({
				kind: 'swap',
				label: `Swapped ${amount} ${from} → ${to}`,
				detail: `received ${fmt(r.received)} ${to}`,
				hash: r.tx_hash
			});
			amount = '';
			quote = null;
			await wallet.refreshBalances();
		} catch (e) {
			toast.error('Swap failed', { description: errMessage(e) });
		} finally {
			proving = false;
		}
	}
</script>

<Workspace title="Swap" subtitle="Convert one shielded asset into another inside the pool">
	{#snippet main()}
		<Card.Root class="max-w-xl">
			<Card.Header>
				<Card.Title>Swap shielded assets</Card.Title>
				<Card.Description>
					An in-pool AMM trade — your funds never leave the shielded pool.
				</Card.Description>
			</Card.Header>
			<Card.Content>
				<Field.Group>
					<Field.Field>
						<Field.Label>From</Field.Label>
						<AssetSelect bind:value={from} />
						<Field.Field>
							<AmountInput
								bind:value={amount}
								code={from}
								decimals={bal?.decimals ?? 7}
								max={bal?.raw}
							/>
							{#if bal}<Field.Description>Available: {bal.display} {from}</Field.Description>{/if}
						</Field.Field>
					</Field.Field>

					<div class="flex justify-center text-muted-foreground">
						<ArrowDownIcon class="size-5" />
					</div>

					<Field.Field>
						<Field.Label>To</Field.Label>
						<AssetSelect bind:value={to} />
						<Field.Description>
							{#if quoting}
								Pricing against the pool…
							{:else if quoteErr}
								<span class="text-destructive">{quoteErr}</span>
							{:else if quote}
								Est. receive <b>{fmt(quote.dest_amount)} {to}</b>
								· min <b>{fmt(minReceived)} {to}</b> after slippage
							{:else}
								Enter an amount to see a quote.
							{/if}
						</Field.Description>
					</Field.Field>

					<Field.Field>
						<Field.Label for="slip">Slippage tolerance (%)</Field.Label>
						<Input id="slip" bind:value={slippagePct} type="number" step="0.1" class="w-32" />
						<Field.Description>
							The swap fails rather than fill below this. Default 1%.
						</Field.Description>
					</Field.Field>
				</Field.Group>
			</Card.Content>
			<Card.Footer>
				<Button onclick={review} disabled={!amount || from === to || !!quoteErr}>Review swap</Button>
			</Card.Footer>
		</Card.Root>
	{/snippet}

	{#snippet aside()}
		<Alert.Root>
			<ShieldCheckIcon />
			<Alert.Title>Stays inside the pool</Alert.Title>
			<Alert.Description>
				The swap spends a shielded note and mints a new one in a single private transaction — no
				public-account hop, no public DEX. Your identity stays hidden. Note: the trade <b>amount</b>
				and asset pair are visible on-chain (the AMM prices the trade on public reserves).
			</Alert.Description>
		</Alert.Root>
	{/snippet}
</Workspace>

<AlertDialog.Root bind:open={confirmOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Confirm swap</AlertDialog.Title>
			<AlertDialog.Description>
				Swap <b>{amount} {from}</b> for at least <b>{fmt(minReceived)} {to}</b> (slippage
				{slippagePct}%)? This is a single in-pool transaction.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={submit}>Swap</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title="Swapping" />
