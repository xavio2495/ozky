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
	import Repeat2Icon from '@lucide/svelte/icons/repeat-2';
	import InfoIcon from '@lucide/svelte/icons/info';
	import { toast } from 'svelte-sonner';
	import { api, errMessage, type Subscription, type Channel } from '$lib/api';
	import { wallet } from '$lib/wallet.svelte';
	import { toBaseUnits } from '$lib/assets';
	import { prettyAmount } from '$lib/format';

	let proving = $state(false);
	let provingTitle = $state('Charging subscription');

	// --- merchant-pull channel state ---
	let chOpen = $state(false); // open-channel dialog
	let chAsset = $state('USDC');
	let chMerchant = $state('');
	let chPerAmount = $state('');
	let chPeriods = $state('12');
	let chCadence = $state('monthly'); // weekly | monthly | days
	let chIntervalDays = $state('30');
	let importOpen = $state(false);
	let importId = $state('');
	let confirmCloseId = $state<number | null>(null);
	let confirmReclaimId = $state<number | null>(null);

	const chBal = $derived(wallet.balances.find((b) => b.code === chAsset));
	const chDecimals = $derived(chBal?.decimals ?? 7);
	const periodSecs = $derived(
		chCadence === 'weekly'
			? 7 * 86400
			: chCadence === 'monthly'
				? 30 * 86400
				: Math.max(1, Number(chIntervalDays) || 1) * 86400
	);
	// The cap a channel locks = the full pre-authorized ramp (per-period * periods).
	const chCap = $derived(
		(Number(chPerAmount) > 0 ? Number(chPerAmount) : 0) * (Number(chPeriods) || 0)
	);

	function openChannelCreate() {
		chAsset = 'USDC';
		chMerchant = '';
		chPerAmount = '';
		chPeriods = '12';
		chCadence = 'monthly';
		chIntervalDays = '30';
		chOpen = true;
	}

	async function doOpenChannel() {
		if (!chMerchant.trim().startsWith('ozky'))
			return toast.error('Enter a valid ozky… merchant code');
		if (!(Number(chPerAmount) > 0)) return toast.error('Enter a per-period amount greater than zero');
		const periods = Number(chPeriods) || 0;
		if (!(periods > 0)) return toast.error('Enter at least one period');
		let perBase: number, capBase: number;
		try {
			perBase = toBaseUnits(chPerAmount, chDecimals);
			capBase = toBaseUnits(String(chCap), chDecimals);
		} catch (e) {
			return toast.error(errMessage(e));
		}
		chOpen = false;
		provingTitle = 'Opening channel';
		proving = true;
		await wallet.openChannel(chAsset, capBase, chMerchant.trim(), perBase, periods, periodSecs);
		proving = false;
	}

	async function doImportChannel() {
		const id = Number(importId);
		if (!Number.isInteger(id) || id < 0) return toast.error('Enter a valid channel id');
		importOpen = false;
		await wallet.importChannel(id);
	}

	async function doCloseChannel() {
		const id = confirmCloseId;
		confirmCloseId = null;
		if (id == null) return;
		provingTitle = `Closing channel #${id}`;
		proving = true;
		await wallet.closeChannel(id);
		proving = false;
	}

	async function doReclaimChannel() {
		const id = confirmReclaimId;
		confirmReclaimId = null;
		if (id == null) return;
		provingTitle = `Reclaiming channel #${id}`;
		proving = true;
		await wallet.reclaimChannel(id);
		proving = false;
	}

	const chDec = (c: Channel) => wallet.balances.find((b) => b.code === c.asset)?.decimals ?? 7;
	const chRole = (c: Channel) =>
		c.is_subscriber && c.is_merchant
			? 'You (both sides)'
			: c.is_subscriber
				? 'You pay'
				: 'You charge';

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

			<!-- Merchant-pull channels (the pull direction) -->
			<div class="mt-6 flex items-center justify-between">
				<div>
					<h2 class="text-sm font-semibold">Merchant-pull channels</h2>
					<p class="text-xs text-muted-foreground">
						Lock a capped amount; a merchant draws each period while you're offline.
					</p>
				</div>
				<div class="flex gap-2">
					<Button variant="outline" size="sm" onclick={() => (importOpen = true)}>Import</Button>
					<Button size="sm" onclick={openChannelCreate} class="gap-2">
						<PlusIcon class="size-4" /> New channel
					</Button>
				</div>
			</div>

			{#if wallet.channels.length === 0}
				<Empty.Root class="rounded-xl border border-dashed py-12">
					<Empty.Header>
						<Empty.Media variant="icon"><Repeat2Icon /></Empty.Media>
						<Empty.Title>No channels yet</Empty.Title>
						<Empty.Description>
							Open a channel to pre-authorize a merchant, or import one you collect on.
						</Empty.Description>
					</Empty.Header>
				</Empty.Root>
			{:else}
				{#each wallet.channels as c (c.id)}
					<Card.Root>
						<Card.Content class="flex items-center gap-4 py-4">
							<div class="min-w-0 flex-1">
								<div class="flex items-center gap-2">
									<span class="font-medium">Channel #{c.id}</span>
									<Badge variant="outline">{chRole(c)}</Badge>
									{#if c.status === 'closed'}<Badge variant="secondary">Closed</Badge>
									{:else if c.expiry_passed}<Badge>Expired</Badge>{/if}
								</div>
								<p class="mt-0.5 text-xs text-muted-foreground">
									cap {prettyAmount(String(c.cap / 10 ** chDec(c)))} {c.asset} ·
									{prettyAmount(String(c.amount_per_period / 10 ** chDec(c)))}/period ·
									drawn so far {prettyAmount(String(c.drawn_so_far / 10 ** chDec(c)))} ·
									expires {fmtDate(c.expiry_unix)}
								</p>
							</div>
							<div class="flex items-center gap-1.5">
								{#if c.is_merchant && c.status === 'open'}
									<Button size="sm" onclick={() => (confirmCloseId = c.id)} disabled={!c.closeable}>
										Close &amp; collect
									</Button>
								{/if}
								{#if c.is_subscriber && c.status === 'open'}
									<Button
										size="sm"
										variant="outline"
										onclick={() => (confirmReclaimId = c.id)}
										disabled={!c.reclaimable}
										title={c.reclaimable ? 'Reclaim the full cap' : 'Reclaimable after expiry'}
									>
										Reclaim
									</Button>
								{/if}
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
		<Alert.Root class="mt-3">
			<Repeat2Icon />
			<Alert.Title>Merchant-pull channels</Alert.Title>
			<Alert.Description>
				<ul class="mt-1 flex list-disc flex-col gap-1 pl-4 text-xs">
					<li>You lock a capped amount and pre-sign each period's charge — the merchant draws while you're offline.</li>
					<li>Amounts stay hidden on-chain; your loss is capped, the unused remainder is always refundable.</li>
					<li>To cancel, ask the merchant to close, or reclaim the full cap after the channel expires.</li>
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

<!-- Open channel (subscriber) -->
<Dialog.Root bind:open={chOpen}>
	<Dialog.Content class="max-w-xl">
		<Dialog.Header>
			<Dialog.Title>New merchant-pull channel</Dialog.Title>
			<Dialog.Description>
				Pre-authorize a merchant to charge a fixed amount each period, up to a cap you lock now.
			</Dialog.Description>
		</Dialog.Header>
		<Field.Group>
			<Field.Field>
				<Field.Label for="chmerchant">Merchant code</Field.Label>
				<Input id="chmerchant" bind:value={chMerchant} placeholder="ozky…" class="font-mono text-sm" />
			</Field.Field>
			<div class="grid grid-cols-2 gap-3">
				<Field.Field>
					<Field.Label>Asset</Field.Label>
					<AssetSelect bind:value={chAsset} />
				</Field.Field>
				<Field.Field>
					<Field.Label for="chper">Amount / period</Field.Label>
					<Input id="chper" bind:value={chPerAmount} inputmode="decimal" placeholder="0.00" class="font-mono" />
				</Field.Field>
			</div>
			<div class="grid grid-cols-2 gap-3">
				<Field.Field>
					<Field.Label>Period</Field.Label>
					<Select.Root type="single" bind:value={chCadence}>
						<Select.Trigger class="h-12 w-full">{cadenceOptions.find((o) => o.value === chCadence)?.label ?? 'Monthly'}</Select.Trigger>
						<Select.Content>
							{#each cadenceOptions as o (o.value)}
								<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</Field.Field>
				<Field.Field>
					<Field.Label for="chperiods">Number of periods</Field.Label>
					<Input id="chperiods" bind:value={chPeriods} inputmode="numeric" placeholder="12" />
				</Field.Field>
			</div>
			{#if chCadence === 'days'}
				<Field.Field>
					<Field.Label for="chinterval">Interval (days)</Field.Label>
					<Input id="chinterval" bind:value={chIntervalDays} inputmode="numeric" placeholder="30" />
				</Field.Field>
			{/if}
			<Alert.Root>
				<InfoIcon />
				<Alert.Description class="text-xs">
					Locks a cap of <strong>{prettyAmount(String(chCap || 0))} {chAsset}</strong>
					({Number(chPeriods) || 0} × {chPerAmount || 0}). The remainder is refundable if the merchant draws less.
				</Alert.Description>
			</Alert.Root>
		</Field.Group>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (chOpen = false)}>Cancel</Button>
			<Button onclick={doOpenChannel}>Open channel</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Import channel (merchant) -->
<Dialog.Root bind:open={importOpen}>
	<Dialog.Content class="max-w-md">
		<Dialog.Header>
			<Dialog.Title>Import a channel</Dialog.Title>
			<Dialog.Description>
				As the merchant, import a channel a subscriber opened to you so you can collect on it.
			</Dialog.Description>
		</Dialog.Header>
		<Field.Field>
			<Field.Label for="impid">Channel id</Field.Label>
			<Input id="impid" bind:value={importId} inputmode="numeric" placeholder="0" />
		</Field.Field>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (importOpen = false)}>Cancel</Button>
			<Button onclick={doImportChannel}>Import</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Close channel confirm (merchant) -->
<AlertDialog.Root open={confirmCloseId !== null} onOpenChange={(o) => { if (!o) confirmCloseId = null; }}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Close and collect?</AlertDialog.Title>
			<AlertDialog.Description>
				Settles the channel at the highest elapsed authorization: mints your draw to you and refunds the remainder to the subscriber. This closes the channel.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={doCloseChannel}>Close &amp; collect</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<!-- Reclaim channel confirm (subscriber) -->
<AlertDialog.Root open={confirmReclaimId !== null} onOpenChange={(o) => { if (!o) confirmReclaimId = null; }}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Reclaim the full cap?</AlertDialog.Title>
			<AlertDialog.Description>
				The channel has expired without the merchant closing it. This mints the entire locked cap back to you and closes the channel.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={doReclaimChannel}>Reclaim</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title={provingTitle} />
