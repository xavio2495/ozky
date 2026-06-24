<script lang="ts">
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import AssetSelect from '$lib/components/shared/AssetSelect.svelte';
	import AmountInput from '$lib/components/shared/AmountInput.svelte';
	import DenominationChips from '$lib/components/shared/DenominationChips.svelte';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import LockIcon from '@lucide/svelte/icons/lock';
	import { toast } from 'svelte-sonner';
	import { api, errMessage } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import { toBaseUnits } from '$lib/assets';
	import { truncate } from '$lib/format';

	let asset = $state('USDC');
	let dest = $state('');
	let amount = $state('');
	let confirmOpen = $state(false);
	let proving = $state(false);

	const bal = $derived(wallet.balances.find((b) => b.code === asset));

	function review() {
		if (!dest.trim().startsWith('G') || dest.trim().length !== 56) {
			toast.error('Enter a valid Stellar G… address');
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
		proving = true;
		const hash = await runAction('Withdrawing', () => api.withdraw(asset, dest.trim(), units), {
			success: () => 'Withdrawal submitted'
		});
		proving = false;
		if (hash) {
			wallet.log({ kind: 'withdraw', label: `Withdrew ${amount} ${asset}`, detail: truncate(dest), hash });
			amount = '';
			dest = '';
		}
	}
</script>

<Workspace title="Withdraw" subtitle="Unshield funds out of the pool to a public Stellar address">
	{#snippet main()}
		<Card.Root class="max-w-xl">
			<Card.Header>
				<Card.Title>Withdraw to public address</Card.Title>
				<Card.Description>The off-ramp: release shielded funds to any Stellar account.</Card.Description>
			</Card.Header>
			<Card.Content>
				<Field.Group>
					<Field.Field>
						<Field.Label>Asset</Field.Label>
						<AssetSelect bind:value={asset} />
					</Field.Field>
					<Field.Field>
						<Field.Label for="dest">Destination</Field.Label>
						<Input id="dest" bind:value={dest} placeholder="G…" class="font-mono" />
						<Field.Description>A standard Stellar public address.</Field.Description>
					</Field.Field>
					<Field.Field>
						<Field.Label>Amount</Field.Label>
						<AmountInput bind:value={amount} code={asset} decimals={bal?.decimals ?? 7} max={bal?.raw} />
						<DenominationChips bind:value={amount} />
						{#if bal}<Field.Description>Available: {bal.display} {asset}</Field.Description>{/if}
					</Field.Field>
				</Field.Group>
			</Card.Content>
			<Card.Footer>
				<Button onclick={review} disabled={!dest || !amount}>Review withdrawal</Button>
			</Card.Footer>
		</Card.Root>
	{/snippet}

	{#snippet aside()}
		<Alert.Root>
			<LockIcon />
			<Alert.Title>Destination-bound proof</Alert.Title>
			<Alert.Description>
				The withdrawal proof is cryptographically bound to this destination — funds can only be
				released to the address you enter, even via a relayer.
			</Alert.Description>
		</Alert.Root>
	{/snippet}
</Workspace>

<AlertDialog.Root bind:open={confirmOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Confirm withdrawal</AlertDialog.Title>
			<AlertDialog.Description>
				Withdraw <b>{amount} {asset}</b> to <span class="font-mono">{truncate(dest)}</span>? This
				unshields the funds — the destination becomes public.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={submit}>Withdraw</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title="Withdrawing" />
