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
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import InfoIcon from '@lucide/svelte/icons/info';
	import { toast } from 'svelte-sonner';
	import { onMount } from 'svelte';
	import { api, errMessage, type Payroll, type KeeperRun } from '$lib/api';
	import { wallet } from '$lib/wallet.svelte';
	import { toBaseUnits, assetByCode, ASSETS } from '$lib/assets';
	import { prettyAmount } from '$lib/format';

	const assetCodes = ASSETS.map((a) => a.code);

	const MAX_PAYEES = 25; // ceil(25/5) = 5 split txs — a generous cap.

	let proving = $state(false);
	let provingTitle = $state('Running payroll');

	// Create/edit dialog state.
	let editOpen = $state(false);
	let editId = $state(0);
	let label = $state('');
	let asset = $state('USDC');
	let cadence = $state('weekly');
	let intervalDays = $state('14');
	// `recv` is the receive asset; '' (or equal to the payroll asset) = same-asset payment.
	let rows = $state<{ code: string; amount: string; recv: string }[]>([
		{ code: '', amount: '', recv: '' }
	]);

	let confirmRunId = $state<number | null>(null);
	let confirmDeleteId = $state<number | null>(null);

	// Headless keeper: armed runs (pre-proved, awaiting a scheduled headless submit).
	let keeperRuns = $state<KeeperRun[]>([]);
	const armedFor = (id: number) => keeperRuns.find((r) => r.payroll_id === id);

	async function refreshKeeper() {
		try {
			keeperRuns = await api.keeperStatus();
		} catch {
			keeperRuns = [];
		}
	}
	onMount(refreshKeeper);

	async function armKeeper(p: Payroll) {
		provingTitle = `Arming headless keeper for "${p.label}"`;
		proving = true;
		try {
			await api.armPayrollKeeper(p.id);
			await refreshKeeper();
			toast.success('Armed for a headless run', {
				description: 'A local task can submit this run on schedule, even with ozky closed.'
			});
		} catch (e) {
			toast.error('Could not arm', { description: errMessage(e) });
		} finally {
			proving = false;
		}
	}

	async function disarmKeeper(p: Payroll) {
		try {
			await api.disarmPayrollKeeper(p.id);
			await refreshKeeper();
			toast.success('Headless run disarmed');
		} catch (e) {
			toast.error('Could not disarm', { description: errMessage(e) });
		}
	}

	const cadenceLabel = (p: Payroll) =>
		p.cadence === 'weekly' ? 'Weekly' : p.cadence === 'monthly' ? 'Monthly' : `Every ${p.interval_days} days`;
	const fmtDate = (unix: number) =>
		new Date(unix * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
	const bal = $derived(wallet.balances.find((b) => b.code === asset));
	const decimals = $derived(bal?.decimals ?? 7);

	function openCreate() {
		editId = 0;
		label = '';
		asset = 'USDC';
		cadence = 'weekly';
		intervalDays = '14';
		rows = [{ code: '', amount: '', recv: '' }];
		editOpen = true;
	}

	function openEdit(p: Payroll) {
		editId = p.id;
		label = p.label;
		asset = p.asset;
		cadence = p.cadence;
		intervalDays = String(p.interval_days || 14);
		rows = p.payees.map((x) => {
			const dec = x.recv_asset ? (assetByCode(x.recv_asset)?.decimals ?? 7) : decimals;
			return { code: x.code, amount: String(x.amount / 10 ** dec), recv: x.recv_asset ?? '' };
		});
		if (rows.length === 0) rows = [{ code: '', amount: '', recv: '' }];
		editOpen = true;
	}

	function addRow() {
		if (rows.length >= MAX_PAYEES) return;
		rows = [...rows, { code: '', amount: '', recv: '' }];
	}
	function removeRow(i: number) {
		rows = rows.filter((_, idx) => idx !== i);
	}

	async function save() {
		const valid = rows.filter((r) => r.code.trim() && Number(r.amount) > 0);
		if (!label.trim()) return toast.error('Give the payroll a name');
		if (valid.length === 0) return toast.error('Add at least one payee');
		let payees;
		try {
			payees = valid.map((r) => {
				if (!r.code.trim().startsWith('ozky')) throw new Error('Each payee needs a valid ozky… code');
				const recvAsset = r.recv && r.recv !== asset ? r.recv : undefined;
				const dec = recvAsset ? (assetByCode(recvAsset)?.decimals ?? 7) : decimals;
				return { code: r.code.trim(), amount: toBaseUnits(r.amount, dec), recv_asset: recvAsset };
			});
		} catch (e) {
			return toast.error(errMessage(e));
		}
		try {
			await api.savePayroll({
				id: editId,
				label: label.trim(),
				asset,
				payees,
				cadence,
				interval_days: cadence === 'days' ? Math.max(1, Number(intervalDays) || 1) : 0,
				start_unix: 0 // default: now → immediately due, so a fresh payroll can be run
			});
			editOpen = false;
			await wallet.refreshPayrolls();
			toast.success(editId ? 'Payroll updated' : 'Payroll created');
		} catch (e) {
			toast.error('Could not save payroll', { description: errMessage(e) });
		}
	}

	async function doRun() {
		const id = confirmRunId;
		confirmRunId = null;
		if (id == null) return;
		const p = wallet.payrolls.find((x) => x.id === id);
		provingTitle = `Running payroll${p ? ` "${p.label}"` : ''}`;
		proving = true;
		await wallet.runPayroll(id);
		proving = false;
	}

	async function toggle(p: Payroll) {
		try {
			await api.setPayrollEnabled(p.id, !p.enabled);
			await wallet.refreshPayrolls();
		} catch (e) {
			toast.error('Could not update', { description: errMessage(e) });
		}
	}

	async function doDelete() {
		const id = confirmDeleteId;
		confirmDeleteId = null;
		if (id == null) return;
		try {
			await api.deletePayroll(id);
			await wallet.refreshPayrolls();
			toast.success('Payroll deleted');
		} catch (e) {
			toast.error('Could not delete', { description: errMessage(e) });
		}
	}

	const cadenceOptions = [
		{ value: 'weekly', label: 'Weekly' },
		{ value: 'monthly', label: 'Monthly' },
		{ value: 'days', label: 'Every N days' }
	];
	const cadenceTrigger = $derived(cadenceOptions.find((o) => o.value === cadence)?.label ?? 'Weekly');
</script>

<Workspace title="Payroll" subtitle="Recurring shielded payouts to a saved group of payees">
	{#snippet main()}
		<div class="flex flex-col gap-4">
			<div class="flex justify-end">
				<Button onclick={openCreate} class="gap-2"><PlusIcon class="size-4" /> New payroll</Button>
			</div>

			{#if wallet.payrolls.length === 0}
				<Empty.Root class="rounded-xl border border-dashed py-16">
					<Empty.Header>
						<Empty.Media variant="icon"><CalendarClockIcon /></Empty.Media>
						<Empty.Title>No payrolls yet</Empty.Title>
						<Empty.Description>Create a payroll to pay a group of recipients on a schedule.</Empty.Description>
					</Empty.Header>
				</Empty.Root>
			{:else}
				{#each wallet.payrolls as p (p.id)}
					<Card.Root>
						<Card.Content class="flex items-center gap-4 py-4">
							<div class="min-w-0 flex-1">
								<div class="flex items-center gap-2">
									<span class="font-medium">{p.label}</span>
									{#if p.due && p.enabled}<Badge>Due</Badge>{/if}
									{#if !p.enabled}<Badge variant="outline">Paused</Badge>{/if}
								</div>
								<p class="mt-0.5 text-xs text-muted-foreground">
									{p.payees.length} payees · {prettyAmount(String(p.total / 10 ** decimals))} {p.asset} · {cadenceLabel(p)}
									· next {fmtDate(p.next_run_unix)}
								</p>
								{#if armedFor(p.id)}
									{@const run = armedFor(p.id)!}
									<p class="mt-1 flex flex-wrap items-center gap-1.5 text-xs">
										<Badge variant="secondary">Headless armed</Badge>
										<span class="text-muted-foreground">
											epoch {run.bound_epoch} · {run.chunks} chunk{run.chunks === 1 ? '' : 's'}{run.submitted
												? ` · ${run.submitted} submitted`
												: ''} — until the epoch rolls or you spend manually
										</span>
										{#if run.error}<span class="text-destructive">· {run.error}</span>{/if}
									</p>
								{/if}
							</div>
							<div class="flex items-center gap-1.5">
								<Button size="sm" onclick={() => (confirmRunId = p.id)} disabled={!p.enabled}>
									<PlayIcon class="size-4" data-icon="inline-start" /> Run now
								</Button>
								{#if armedFor(p.id)}
									<Button variant="ghost" size="sm" onclick={() => disarmKeeper(p)} title="Stop headless runs">
										Disarm
									</Button>
								{:else}
									<Button variant="outline" size="sm" onclick={() => armKeeper(p)} disabled={!p.enabled}>
										Run headless
									</Button>
								{/if}
								<Button variant="ghost" size="icon" onclick={() => toggle(p)} title={p.enabled ? 'Pause' : 'Resume'}>
									{#if p.enabled}<PauseIcon class="size-4" />{:else}<PlayIcon class="size-4" />{/if}
								</Button>
								<Button variant="ghost" size="sm" onclick={() => openEdit(p)}>Edit</Button>
								<Button variant="ghost" size="icon" onclick={() => (confirmDeleteId = p.id)} title="Delete">
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
			<Alert.Title>How payroll runs</Alert.Title>
			<Alert.Description>
				<ul class="mt-1 flex list-disc flex-col gap-1 pl-4 text-xs">
					<li>Runs only while ozky is open — due payrolls are flagged for you to run.</li>
					<li>Each run pays same-asset payees in batches of 5; cross-asset payees are individual txs.</li>
						<li><b>Run headless</b> pre-proves the next run so a background task can submit it on schedule even with ozky closed — valid only until the epoch rolls or you spend manually (re-arm on open).</li>
					<li>Nothing is paid without your explicit "Run now".</li>
				</ul>
			</Alert.Description>
		</Alert.Root>
	{/snippet}
</Workspace>

<!-- Create / edit payroll -->
<Dialog.Root bind:open={editOpen}>
	<Dialog.Content class="max-w-xl">
		<Dialog.Header>
			<Dialog.Title>{editId ? 'Edit payroll' : 'New payroll'}</Dialog.Title>
			<Dialog.Description>Pay a saved group of recipients on a recurring schedule.</Dialog.Description>
		</Dialog.Header>
		<Field.Group>
			<Field.Field>
				<Field.Label for="pname">Name</Field.Label>
				<Input id="pname" bind:value={label} placeholder="e.g. Engineering team" />
			</Field.Field>
			<div class="grid grid-cols-2 gap-3">
				<Field.Field>
					<Field.Label>Asset</Field.Label>
					<AssetSelect bind:value={asset} />
				</Field.Field>
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
			</div>
			{#if cadence === 'days'}
				<Field.Field>
					<Field.Label for="interval">Interval (days)</Field.Label>
					<Input id="interval" bind:value={intervalDays} inputmode="numeric" placeholder="14" />
				</Field.Field>
			{/if}
			<Field.Field>
				<Field.Label>Payees ({rows.length})</Field.Label>
				<div class="flex flex-col gap-2">
					{#each rows as row, i (i)}
						<div class="flex items-center gap-2">
							<Input bind:value={row.code} placeholder="ozky…" class="flex-1 font-mono text-sm" />
							<Input bind:value={row.amount} inputmode="decimal" placeholder="0.00" class="w-24 font-mono" />
							<Select.Root type="single" value={row.recv || asset} onValueChange={(v) => (row.recv = v)}>
								<Select.Trigger class="h-9 w-20" title="Receive asset">{row.recv || asset}</Select.Trigger>
								<Select.Content>
									{#each assetCodes as code (code)}
										<Select.Item value={code} label={code}>{code}</Select.Item>
									{/each}
								</Select.Content>
							</Select.Root>
							<Button variant="ghost" size="icon" onclick={() => removeRow(i)} disabled={rows.length <= 1} aria-label="Remove">
								<Trash2Icon class="size-4" />
							</Button>
						</div>
					{/each}
				</div>
				<Field.Description>
					Pay each payee in {asset}. Pick a different "receives" asset to pay them across assets in-pool
					(then the amount is what they receive).
				</Field.Description>
				<Button variant="outline" size="sm" class="mt-1 gap-2 self-start" onclick={addRow} disabled={rows.length >= MAX_PAYEES}>
					<PlusIcon class="size-4" /> Add payee
				</Button>
			</Field.Field>
		</Field.Group>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (editOpen = false)}>Cancel</Button>
			<Button onclick={save}>{editId ? 'Save changes' : 'Create payroll'}</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Run confirm -->
<AlertDialog.Root open={confirmRunId !== null} onOpenChange={(o) => { if (!o) confirmRunId = null; }}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Run this payroll now?</AlertDialog.Title>
			<AlertDialog.Description>
				This pays all payees in one or more shielded transactions and advances the schedule.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={doRun}>Run now</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<!-- Delete confirm -->
<AlertDialog.Root open={confirmDeleteId !== null} onOpenChange={(o) => { if (!o) confirmDeleteId = null; }}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete payroll?</AlertDialog.Title>
			<AlertDialog.Description>This removes the saved schedule. It does not affect past payments.</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={doDelete}>Delete</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title={provingTitle} />
