<script lang="ts">
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import AssetSelect from '$lib/components/shared/AssetSelect.svelte';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import SplitIcon from '@lucide/svelte/icons/split';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';
	import { toast } from 'svelte-sonner';
	import { api, errMessage } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import * as Select from '$lib/components/ui/select';
	import { toBaseUnits, assetByCode, ASSETS } from '$lib/assets';
	import { prettyAmount } from '$lib/format';

	const MAX_RECIPIENTS = 5;
	const assetCodes = ASSETS.map((a) => a.code);

	let asset = $state('USDC');
	// `recv` is the receive asset; '' (or equal to `asset`) = same-asset, bundled into the split.
	let rows = $state<{ recipient: string; amount: string; recv: string }[]>([
		{ recipient: '', amount: '', recv: '' },
		{ recipient: '', amount: '', recv: '' }
	]);
	let confirmOpen = $state(false);
	let proving = $state(false);

	const bal = $derived(wallet.balances.find((b) => b.code === asset));
	const decimals = $derived(bal?.decimals ?? 7);
	const totalDisplay = $derived(
		rows.reduce((s, r) => s + (Number(r.amount) || 0), 0)
	);
	const validRows = $derived(
		rows.filter((r) => r.recipient.trim() && Number(r.amount) > 0)
	);
	// Any recipient receiving a different asset → cross-asset legs (separate in-pool pay txs).
	const hasCrossAsset = $derived(validRows.some((r) => r.recv && r.recv !== asset));

	function addRow() {
		if (rows.length >= MAX_RECIPIENTS) return;
		rows = [...rows, { recipient: '', amount: '', recv: '' }];
	}
	function removeRow(i: number) {
		rows = rows.filter((_, idx) => idx !== i);
	}

	function review() {
		if (validRows.length === 0) {
			toast.error('Add at least one recipient with an amount');
			return;
		}
		for (const r of validRows) {
			if (!r.recipient.trim().startsWith('ozky')) {
				toast.error('Each recipient must be a valid ozky… code');
				return;
			}
			const recvAsset = r.recv && r.recv !== asset ? r.recv : undefined;
			try {
				toBaseUnits(r.amount, recvAsset ? (assetByCode(recvAsset)?.decimals ?? 7) : decimals);
			} catch (e) {
				toast.error(errMessage(e));
				return;
			}
		}
		// Only the same-asset total can be checked against the balance up front (cross-asset cost is
		// quoted in `asset` at submit time); the backend rejects an over-spend either way.
		if (!hasCrossAsset && bal && totalDisplay > Number(bal.display)) {
			toast.error('Split total exceeds your shielded balance');
			return;
		}
		confirmOpen = true;
	}

	async function submit() {
		confirmOpen = false;
		const recipients = validRows.map((r) => {
			const recvAsset = r.recv && r.recv !== asset ? r.recv : undefined;
			const dec = recvAsset ? (assetByCode(recvAsset)?.decimals ?? 7) : decimals;
			return { recipient: r.recipient.trim(), amount: toBaseUnits(r.amount, dec), recv_asset: recvAsset };
		});
		proving = true;
		// Pure same-asset → one shielded split tx (unchanged). Any cross-asset recipient → multi_send
		// (same-asset bundled into a split, each cross-asset recipient an individual in-pool pay).
		const hash = await runAction(
			'Splitting payment',
			() =>
				hasCrossAsset
					? api.multiSend(asset, recipients).then((hs) => hs[0])
					: api.split(
							asset,
							recipients.map(({ recipient, amount }) => ({ recipient, amount }))
						),
			{ success: () => 'Split sent' }
		);
		proving = false;
		if (hash) {
			wallet.log({
				kind: 'split',
				label: `Split ${totalDisplay} ${asset}`,
				detail: `${recipients.length} recipients${hasCrossAsset ? ' (cross-asset)' : ''}`,
				hash
			});
			rows = [
				{ recipient: '', amount: '', recv: '' },
				{ recipient: '', amount: '', recv: '' }
			];
		}
	}
</script>

<Workspace title="Split" subtitle="Pay up to 5 recipients in one shielded transaction">
	{#snippet main()}
		<Card.Root class="max-w-2xl">
			<Card.Header>
				<Card.Title>New split payment</Card.Title>
				<Card.Description>One private transfer, many recipients. Change returns to you.</Card.Description>
			</Card.Header>
			<Card.Content>
				<Field.Group>
					<Field.Field>
						<Field.Label>Asset</Field.Label>
						<AssetSelect bind:value={asset} />
						{#if bal}<Field.Description>Available: {bal.display} {asset}</Field.Description>{/if}
					</Field.Field>

					<Field.Field>
						<Field.Label>Recipients ({validRows.length}/{MAX_RECIPIENTS})</Field.Label>
						<div class="flex flex-col gap-2">
							{#each rows as row, i (i)}
								<div class="flex items-center gap-2">
									<Input bind:value={row.recipient} placeholder="ozky…" class="flex-1 font-mono text-sm" />
									<Input
										bind:value={row.amount}
										inputmode="decimal"
										placeholder="0.00"
										class="w-28 font-mono"
									/>
									<Select.Root type="single" value={row.recv || asset} onValueChange={(v) => (row.recv = v)}>
										<Select.Trigger class="h-9 w-20" title="Receive asset">{row.recv || asset}</Select.Trigger>
										<Select.Content>
											{#each assetCodes as code (code)}
												<Select.Item value={code} label={code}>{code}</Select.Item>
											{/each}
										</Select.Content>
									</Select.Root>
									<Button
										variant="ghost"
										size="icon"
										onclick={() => removeRow(i)}
										disabled={rows.length <= 1}
										aria-label="Remove recipient"
									>
										<Trash2Icon class="size-4" />
									</Button>
								</div>
							{/each}
						</div>
						<Field.Description>
							Each recipient receives {asset} by default. Pick a different asset to pay them across
							assets in-pool (amount = what they receive); cross-asset recipients are paid as separate
							transactions.
						</Field.Description>
						<Button
							variant="outline"
							size="sm"
							class="mt-1 gap-2 self-start"
							onclick={addRow}
							disabled={rows.length >= MAX_RECIPIENTS}
						>
							<PlusIcon class="size-4" /> Add recipient
						</Button>
					</Field.Field>
				</Field.Group>
			</Card.Content>
			<Card.Footer class="justify-between">
				<span class="text-sm text-muted-foreground">
					Total: <span class="font-mono font-medium text-foreground">{prettyAmount(String(totalDisplay))} {asset}</span>
				</span>
				<Button onclick={review} disabled={validRows.length === 0}>
					<SplitIcon class="size-4" data-icon="inline-start" />
					Review split
				</Button>
			</Card.Footer>
		</Card.Root>
	{/snippet}

	{#snippet aside()}
		<Alert.Root>
			<EyeOffIcon />
			<Alert.Title>One shielded transaction</Alert.Title>
			<Alert.Description>
				<ul class="mt-1 flex list-disc flex-col gap-1 pl-4 text-xs">
					<li>All recipients are paid in a single private transfer.</li>
					<li>The output count is always padded to 6 — observers can't tell how many recipients you paid.</li>
					<li>The relayer pays the network fee; your XLM is untouched.</li>
				</ul>
			</Alert.Description>
		</Alert.Root>
	{/snippet}
</Workspace>

<AlertDialog.Root bind:open={confirmOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Confirm split</AlertDialog.Title>
			<AlertDialog.Description>
				Send <b>{prettyAmount(String(totalDisplay))} {asset}</b> across
				<b>{validRows.length}</b> recipients{#if hasCrossAsset}, with cross-asset recipients paid as
				separate in-pool transactions{:else} in one shielded transaction{/if}?
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={submit}>Send split</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title="Splitting payment" />
