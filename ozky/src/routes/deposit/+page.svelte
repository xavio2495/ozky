<script lang="ts">
	import { onMount } from 'svelte';
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import AssetSelect from '$lib/components/shared/AssetSelect.svelte';
	import AmountInput from '$lib/components/shared/AmountInput.svelte';
	import AddressField from '$lib/components/shared/AddressField.svelte';
	import DenominationChips from '$lib/components/shared/DenominationChips.svelte';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import { toast } from 'svelte-sonner';
	import { api, errMessage } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import { toBaseUnits } from '$lib/assets';

	let asset = $state('USDC');
	let amount = $state('');
	let proving = $state(false);
	let funding = $state('');

	const bal = $derived(wallet.balances.find((b) => b.code === asset));

	onMount(async () => {
		try {
			funding = await api.fundingAddress();
		} catch {
			/* shown in settings */
		}
	});

	async function submit() {
		let units: number;
		try {
			units = toBaseUnits(amount, bal?.decimals ?? 7);
		} catch (e) {
			toast.error(errMessage(e));
			return;
		}
		proving = true;
		const hash = await runAction('Shielding deposit', () => api.deposit(asset, units), {
			success: () => 'Deposit shielded'
		});
		proving = false;
		if (hash) {
			wallet.log({ kind: 'deposit', label: `Deposited ${amount} ${asset}`, hash });
			amount = '';
		}
	}
</script>

<Workspace title="Deposit" subtitle="Shield public funds from your Stellar account into the pool">
	{#snippet main()}
		<Card.Root class="max-w-xl">
			<Card.Header>
				<Card.Title>Shield funds</Card.Title>
				<Card.Description>Move funds from your public account into the shielded pool.</Card.Description>
			</Card.Header>
			<Card.Content>
				<Field.Group>
					<Field.Field>
						<Field.Label>Asset</Field.Label>
						<AssetSelect bind:value={asset} />
					</Field.Field>
					<Field.Field>
						<Field.Label>Amount</Field.Label>
						<AmountInput bind:value={amount} code={asset} decimals={bal?.decimals ?? 7} />
						<DenominationChips bind:value={amount} />
						<Field.Description>Must be held in your public funding account first.</Field.Description>
					</Field.Field>
				</Field.Group>
			</Card.Content>
			<Card.Footer>
				<Button onclick={submit} disabled={!amount}>Deposit</Button>
			</Card.Footer>
		</Card.Root>
	{/snippet}

	{#snippet aside()}
		<div class="flex flex-col gap-3">
			<h2 class="text-sm font-medium text-muted-foreground">Fund this account first</h2>
			<AddressField
				label="Funding address"
				value={funding}
				loading={!funding}
				hint="Send public funds here from any wallet or exchange, then deposit to shield them."
				qr
			/>
		</div>
	{/snippet}
</Workspace>

<ProvingOverlay open={proving} title="Shielding deposit" />
