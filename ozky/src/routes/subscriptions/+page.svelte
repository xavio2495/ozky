<script lang="ts">
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import AssetSelect from '$lib/components/shared/AssetSelect.svelte';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Select from '$lib/components/ui/select';
	import * as Alert from '$lib/components/ui/alert';
	import * as Empty from '$lib/components/ui/empty';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import PlayIcon from '@lucide/svelte/icons/play';
	import PauseIcon from '@lucide/svelte/icons/pause';
	import RepeatIcon from '@lucide/svelte/icons/repeat';
	import InfoIcon from '@lucide/svelte/icons/info';
	import { toast } from 'svelte-sonner';
	import { api, errMessage, type Subscription } from '$lib/api';
	import { wallet } from '$lib/wallet.svelte';
	import { toBaseUnits } from '$lib/assets';
	import { prettyAmount } from '$lib/format';

	let proving = $state(false);
	let provingTitle = $state('Charging subscription');

	// Create/edit dialog state.
	let editOpen = $state(false);
	let editId = $state(0);
	let label = $state('');
	let asset = $state('USDC');
	let code = $state('');
	let amount = $state('');
	let cadence = $state('monthly');
	let intervalDays = $state('30');
	let endDate = $state(''); // YYYY-MM-DD; empty = no end

	let confirmRunId = $state<number | null>(null);
	let confirmDeleteId = $state<number | null>(null);

	const cadenceLabel = (s: Subscription) =>
		s.cadence === 'weekly' ? 'Weekly' : s.cadence === 'monthly' ? 'Monthly' : `Every ${s.interval_days} days`;
	const fmtDate = (unix: number) =>
		new Date(unix * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
	const bal = $derived(wallet.balances.find((b) => b.code === asset));
	const decimals = $derived(bal?.decimals ?? 7);

	function openCreate() {
		editId = 0;
		label = '';
		asset = 'USDC';
		code = '';
		amount = '';
		cadence = 'monthly';
		intervalDays = '30';
		endDate = '';
		editOpen = true;
	}

	function openEdit(s: Subscription) {
		editId = s.id;
		label = s.label;
		asset = s.asset;
		code = s.code;
		amount = String(s.amount / 10 ** decimals);
		cadence = s.cadence;
		intervalDays = String(s.interval_days || 30);
		endDate = s.end_unix ? new Date(s.end_unix * 1000).toISOString().slice(0, 10) : '';
		editOpen = true;
	}

	async function save() {
		if (!label.trim()) return toast.error('Give the subscription a name');
		if (!code.trim().startsWith('ozky')) return toast.error('Enter a valid ozky… recipient code');
		if (!(Number(amount) > 0)) return toast.error('Enter an amount greater than zero');
		let base: number;
		try {
			base = toBaseUnits(amount, decimals);
		} catch (e) {
			return toast.error(errMessage(e));
		}
		try {
			await api.saveSubscription({
				id: editId,
				label: label.trim(),
				asset,
				code: code.trim(),
				amount: base,
				cadence,
				interval_days: cadence === 'days' ? Math.max(1, Number(intervalDays) || 1) : 0,
				start_unix: 0, // default: now → immediately due, so a fresh subscription can be run
				end_unix: endDate ? Math.floor(new Date(endDate).getTime() / 1000) : 0
			});
			editOpen = false;
			await wallet.refreshSubscriptions();
			toast.success(editId ? 'Subscription updated' : 'Subscription created');
		} catch (e) {
			toast.error('Could not save subscription', { description: errMessage(e) });
		}
	}

	async function doRun() {
		const id = confirmRunId;
		confirmRunId = null;
		if (id == null) return;
		const s = wallet.subscriptions.find((x) => x.id === id);
		provingTitle = `Charging subscription${s ? ` "${s.label}"` : ''}`;
		proving = true;
		await wallet.runSubscription(id);
		proving = false;
	}

	async function toggle(s: Subscription) {
		try {
			await api.setSubscriptionEnabled(s.id, !s.enabled);
			await wallet.refreshSubscriptions();
		} catch (e) {
			toast.error('Could not update', { description: errMessage(e) });
		}
	}

	async function doDelete() {
		const id = confirmDeleteId;
		confirmDeleteId = null;
		if (id == null) return;
		try {
			await api.deleteSubscription(id);
			await wallet.refreshSubscriptions();
			toast.success('Subscription deleted');
		} catch (e) {
			toast.error('Could not delete', { description: errMessage(e) });
		}
	}

	const cadenceOptions = [
		{ value: 'weekly', label: 'Weekly' },
		{ value: 'monthly', label: 'Monthly' },
		{ value: 'days', label: 'Every N days' }
	];
	const cadenceTrigger = $derived(cadenceOptions.find((o) => o.value === cadence)?.label ?? 'Monthly');
</script>

<Workspace title="Subscriptions" subtitle="Recurring shielded payments to a single recipient">
	{#snippet main()}
		<div class="flex flex-col gap-4">
			<div class="flex justify-end">
				<Button onclick={openCreate} class="gap-2"><PlusIcon class="size-4" /> New subscription</Button>
			</div>

			{#if wallet.subscriptions.length === 0}
				<Empty.Root class="rounded-xl border border-dashed py-16">
					<Empty.Header>
						<Empty.Media variant="icon"><RepeatIcon /></Empty.Media>
						<Empty.Title>No subscriptions yet</Empty.Title>
						<Empty.Description>Set up a recurring payment to a recipient you pay on a schedule.</Empty.Description>
					</Empty.Header>
				</Empty.Root>
			{:else}
				{#each wallet.subscriptions as s (s.id)}
					<Card.Root>
						<Card.Content class="flex items-center gap-4 py-4">
							<div class="min-w-0 flex-1">
								<div class="flex items-center gap-2">
									<span class="font-medium">{s.label}</span>
									{#if s.due && s.enabled}<Badge>Due</Badge>{/if}
									{#if !s.enabled}<Badge variant="outline">Paused</Badge>{/if}
								</div>
								<p class="mt-0.5 text-xs text-muted-foreground">
									{prettyAmount(String(s.amount / 10 ** decimals))} {s.asset} · {cadenceLabel(s)} · next {fmtDate(s.next_run_unix)}
									{#if s.end_unix}· ends {fmtDate(s.end_unix)}{/if}
								</p>
							</div>
							<div class="flex items-center gap-1.5">
								<Button size="sm" onclick={() => (confirmRunId = s.id)} disabled={!s.enabled}>
									<PlayIcon class="size-4" data-icon="inline-start" /> Pay now
								</Button>
								<Button variant="ghost" size="icon" onclick={() => toggle(s)} title={s.enabled ? 'Pause' : 'Resume'}>
									{#if s.enabled}<PauseIcon class="size-4" />{:else}<PlayIcon class="size-4" />{/if}
								</Button>
								<Button variant="ghost" size="sm" onclick={() => openEdit(s)}>Edit</Button>
								<Button variant="ghost" size="icon" onclick={() => (confirmDeleteId = s.id)} title="Delete">
									<Trash2Icon class="size-4" />
								</Button>
							</div>
						</Card.Content>
					</Card.Root>
				{/each}
			{/if}
		</div>
	{/snippet}

	{#snippet aside()}
		<Alert.Root>
			<InfoIcon />
			<Alert.Title>How subscriptions run</Alert.Title>
			<Alert.Description>
				<ul class="mt-1 flex list-disc flex-col gap-1 pl-4 text-xs">
					<li>Runs only while ozky is open — due subscriptions are flagged for you to pay.</li>
					<li>Each charge is one shielded transfer to the recipient.</li>
					<li>This is a push (you pay) subscription — cancel any time by deleting it.</li>
				</ul>
			</Alert.Description>
		</Alert.Root>
	{/snippet}
</Workspace>

<!-- Create / edit subscription -->
<Dialog.Root bind:open={editOpen}>
	<Dialog.Content class="max-w-xl">
		<Dialog.Header>
			<Dialog.Title>{editId ? 'Edit subscription' : 'New subscription'}</Dialog.Title>
			<Dialog.Description>Pay a recipient a fixed amount on a recurring schedule.</Dialog.Description>
		</Dialog.Header>
		<Field.Group>
			<Field.Field>
				<Field.Label for="sname">Name</Field.Label>
				<Input id="sname" bind:value={label} placeholder="e.g. Cloud hosting" />
			</Field.Field>
			<Field.Field>
				<Field.Label for="scode">Recipient code</Field.Label>
				<Input id="scode" bind:value={code} placeholder="ozky…" class="font-mono text-sm" />
			</Field.Field>
			<div class="grid grid-cols-2 gap-3">
				<Field.Field>
					<Field.Label>Asset</Field.Label>
					<AssetSelect bind:value={asset} />
				</Field.Field>
				<Field.Field>
					<Field.Label for="samount">Amount</Field.Label>
					<Input id="samount" bind:value={amount} inputmode="decimal" placeholder="0.00" class="font-mono" />
				</Field.Field>
			</div>
			<div class="grid grid-cols-2 gap-3">
				<Field.Field>
					<Field.Label>Cadence</Field.Label>
					<Select.Root type="single" bind:value={cadence}>
						<Select.Trigger class="h-12 w-full">{cadenceTrigger}</Select.Trigger>
						<Select.Content>
							{#each cadenceOptions as o (o.value)}
								<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</Field.Field>
				<Field.Field>
					<Field.Label for="send">Ends (optional)</Field.Label>
					<Input id="send" type="date" bind:value={endDate} />
				</Field.Field>
			</div>
			{#if cadence === 'days'}
				<Field.Field>
					<Field.Label for="interval">Interval (days)</Field.Label>
					<Input id="interval" bind:value={intervalDays} inputmode="numeric" placeholder="30" />
				</Field.Field>
			{/if}
		</Field.Group>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (editOpen = false)}>Cancel</Button>
			<Button onclick={save}>{editId ? 'Save changes' : 'Create subscription'}</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Pay confirm -->
<AlertDialog.Root open={confirmRunId !== null} onOpenChange={(o) => { if (!o) confirmRunId = null; }}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Pay this subscription now?</AlertDialog.Title>
			<AlertDialog.Description>
				This sends one shielded transfer to the recipient and advances the schedule.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={doRun}>Pay now</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<!-- Delete confirm -->
<AlertDialog.Root open={confirmDeleteId !== null} onOpenChange={(o) => { if (!o) confirmDeleteId = null; }}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete subscription?</AlertDialog.Title>
			<AlertDialog.Description>This removes the saved schedule. It does not affect past payments.</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={doDelete}>Delete</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title={provingTitle} />
