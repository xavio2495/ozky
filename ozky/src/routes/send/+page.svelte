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
	import { toast } from 'svelte-sonner';
	import { api, errMessage } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import { toBaseUnits } from '$lib/assets';
	import { truncate } from '$lib/format';

	let asset = $state('USDC');
	let recipient = $state('');
	let amount = $state('');
	let confirmOpen = $state(false);
	let proving = $state(false);

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

<ProvingOverlay open={proving} title="Sending payment" />
