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
	import HandCoinsIcon from '@lucide/svelte/icons/hand-coins';
	import InfoIcon from '@lucide/svelte/icons/info';
	import { toast } from 'svelte-sonner';
	import { api, errMessage, type Escrow } from '$lib/api';
	import { wallet } from '$lib/wallet.svelte';
	import { toBaseUnits } from '$lib/assets';
	import { prettyAmount } from '$lib/format';

	let proving = $state(false);
	let provingTitle = $state('Working');

	// Open-escrow dialog.
	let openDialog = $state(false);
	let oAsset = $state('USDC');
	let oTarget = $state('');
	let oDeadline = $state(''); // YYYY-MM-DD
	let oMode = $state('all_or_nothing');

	// Contribute dialog.
	let contribDialog = $state(false);
	let cId = $state('');
	let cCode = $state('');
	let cAmount = $state('');

	let confirmRelease = $state<number | null>(null);
	let confirmRefund = $state<{ id: number; index: number } | null>(null);

	const decimals = (code: string) => wallet.balances.find((b) => b.code === code)?.decimals ?? 7;
	const fmtAmount = (base: number, code: string) => prettyAmount(String(base / 10 ** decimals(code)));
	const fmtDate = (unix: number) =>
		new Date(unix * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
	const modeLabel = (m: string) => (m === 'keep_what_you_raise' ? 'Keep what you raise' : 'All or nothing');
	const role = (e: Escrow) =>
		e.is_payee && e.my_contributions.length ? 'Payee · Contributor' : e.is_payee ? 'Payee' : 'Contributor';

	const modeOptions = [
		{ value: 'all_or_nothing', label: 'All or nothing' },
		{ value: 'keep_what_you_raise', label: 'Keep what you raise' }
	];
	const modeTrigger = $derived(modeOptions.find((o) => o.value === oMode)?.label ?? 'All or nothing');

	function openCreate() {
		oAsset = 'USDC';
		oTarget = '';
		oDeadline = '';
		oMode = 'all_or_nothing';
		openDialog = true;
	}

	async function doOpen() {
		if (!(Number(oTarget) > 0)) return toast.error('Enter a target greater than zero');
		if (!oDeadline) return toast.error('Pick a deadline');
		let base: number;
		try {
			base = toBaseUnits(oTarget, decimals(oAsset));
		} catch (e) {
			return toast.error(errMessage(e));
		}
		const deadlineUnix = Math.floor(new Date(oDeadline).getTime() / 1000);
		if (deadlineUnix <= Date.now() / 1000) return toast.error('Deadline must be in the future');
		openDialog = false;
		await wallet.openEscrow(oAsset, base, deadlineUnix, oMode);
	}

	function openContribute() {
		cId = '';
		cCode = '';
		cAmount = '';
		contribDialog = true;
	}

	async function doContribute() {
		const id = Number(cId);
		if (!(id > 0)) return toast.error('Enter the escrow id');
		if (!cCode.trim().startsWith('ozky')) return toast.error('Enter the payee’s ozky… code');
		if (!(Number(cAmount) > 0)) return toast.error('Enter an amount greater than zero');
		// All v1 assets share 7 decimals; the on-chain escrow fixes the asset.
		let base: number;
		try {
			base = toBaseUnits(cAmount, 7);
		} catch (e) {
			return toast.error(errMessage(e));
		}
		contribDialog = false;
		provingTitle = `Contributing to escrow #${id}`;
		proving = true;
		await wallet.contributeEscrow(id, cCode.trim(), base);
		proving = false;
	}

	async function doRelease() {
		const id = confirmRelease;
		confirmRelease = null;
		if (id == null) return;
		provingTitle = `Releasing escrow #${id}`;
		proving = true;
		await wallet.releaseEscrow(id);
		proving = false;
	}

	async function doRefund() {
		const r = confirmRefund;
		confirmRefund = null;
		if (!r) return;
		provingTitle = `Refunding escrow #${r.id}`;
		proving = true;
		await wallet.refundEscrow(r.id, r.index);
		proving = false;
	}
</script>

<Workspace title="Escrow" subtitle="Hidden-sum invoices — many contributors, one shielded payout">
	{#snippet main()}
		<div class="flex flex-col gap-4">
			<div class="flex justify-end gap-2">
				<Button variant="outline" onclick={openContribute} class="gap-2">
					<HandCoinsIcon class="size-4" /> Contribute
				</Button>
				<Button onclick={openCreate} class="gap-2"><PlusIcon class="size-4" /> Open escrow</Button>
			</div>

			{#if wallet.escrows.length === 0}
				<Empty.Root class="rounded-xl border border-dashed py-16">
					<Empty.Header>
						<Empty.Media variant="icon"><HandCoinsIcon /></Empty.Media>
						<Empty.Title>No escrows yet</Empty.Title>
						<Empty.Description>
							Open an escrow to collect a hidden-sum payment, or contribute to one with its id + payee code.
						</Empty.Description>
					</Empty.Header>
				</Empty.Root>
			{:else}
				{#each wallet.escrows as e (e.id)}
					<Card.Root>
						<Card.Content class="flex flex-col gap-3 py-4">
							<div class="flex items-start gap-4">
								<div class="min-w-0 flex-1">
									<div class="flex flex-wrap items-center gap-2">
										<span class="font-medium">Escrow #{e.id}</span>
										<Badge variant="outline">{role(e)}</Badge>
										<Badge variant="secondary">{modeLabel(e.mode)}</Badge>
										{#if e.status === 'released'}<Badge>Released</Badge>{/if}
										{#if e.deadline_passed && e.status === 'open'}<Badge variant="destructive">Ended</Badge>{/if}
									</div>
									<p class="mt-1 text-xs text-muted-foreground">
										Target {fmtAmount(e.target, e.asset)} {e.asset} · {e.n_contrib} contribution{e.n_contrib === 1 ? '' : 's'} ·
										{e.deadline_passed ? 'ended' : 'ends'} {fmtDate(e.deadline_unix)}
									</p>
									{#if e.is_payee && e.raised !== null}
										<p class="mt-0.5 text-xs">
											<span class="text-muted-foreground">Raised (your view):</span>
											<span class="font-medium">{fmtAmount(e.raised, e.asset)} {e.asset}</span>
										</p>
									{/if}
									{#each e.my_contributions as c (c.index)}
										<p class="mt-0.5 text-xs text-muted-foreground">
											Your contribution #{c.index}: {fmtAmount(c.amount, e.asset)} {e.asset}
										</p>
									{/each}
								</div>
								<div class="flex shrink-0 flex-col items-end gap-1.5">
									{#if e.releasable}
										<Button size="sm" onclick={() => (confirmRelease = e.id)}>Release</Button>
									{/if}
									{#if e.refundable}
										{#each e.my_contributions as c (c.index)}
											<Button
												variant="outline"
												size="sm"
												onclick={() => (confirmRefund = { id: e.id, index: c.index })}
											>
												Refund #{c.index}
											</Button>
										{/each}
									{/if}
								</div>
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
			<Alert.Title>How escrow works</Alert.Title>
			<Alert.Description>
				<ul class="mt-1 flex list-disc flex-col gap-1 pl-4 text-xs">
					<li>Contribution amounts are hidden on-chain — only the payee can total them.</li>
					<li><strong>All or nothing:</strong> release once the target is reached; refunds open after the deadline.</li>
					<li><strong>Keep what you raise:</strong> release any amount once the deadline passes.</li>
					<li>Share your escrow id + receive code with contributors so they can pay in.</li>
				</ul>
			</Alert.Description>
		</Alert.Root>
	{/snippet}
</Workspace>

<!-- Open escrow -->
<Dialog.Root bind:open={openDialog}>
	<Dialog.Content class="max-w-xl">
		<Dialog.Header>
			<Dialog.Title>Open escrow</Dialog.Title>
			<Dialog.Description>Collect a hidden-sum payment from one or more contributors.</Dialog.Description>
		</Dialog.Header>
		<Field.Group>
			<div class="grid grid-cols-2 gap-3">
				<Field.Field>
					<Field.Label>Asset</Field.Label>
					<AssetSelect bind:value={oAsset} />
				</Field.Field>
				<Field.Field>
					<Field.Label for="otarget">Target amount</Field.Label>
					<Input id="otarget" bind:value={oTarget} inputmode="decimal" placeholder="0.00" class="font-mono" />
				</Field.Field>
			</div>
			<div class="grid grid-cols-2 gap-3">
				<Field.Field>
					<Field.Label>Mode</Field.Label>
					<Select.Root type="single" bind:value={oMode}>
						<Select.Trigger class="h-12 w-full">{modeTrigger}</Select.Trigger>
						<Select.Content>
							{#each modeOptions as o (o.value)}
								<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</Field.Field>
				<Field.Field>
					<Field.Label for="odeadline">Deadline</Field.Label>
					<Input id="odeadline" type="date" bind:value={oDeadline} />
				</Field.Field>
			</div>
		</Field.Group>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (openDialog = false)}>Cancel</Button>
			<Button onclick={doOpen}>Open escrow</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Contribute -->
<Dialog.Root bind:open={contribDialog}>
	<Dialog.Content class="max-w-xl">
		<Dialog.Header>
			<Dialog.Title>Contribute to an escrow</Dialog.Title>
			<Dialog.Description>Your amount stays hidden; only the payee can total contributions.</Dialog.Description>
		</Dialog.Header>
		<Field.Group>
			<div class="grid grid-cols-2 gap-3">
				<Field.Field>
					<Field.Label for="cid">Escrow id</Field.Label>
					<Input id="cid" bind:value={cId} inputmode="numeric" placeholder="e.g. 3" />
				</Field.Field>
				<Field.Field>
					<Field.Label for="camount">Amount</Field.Label>
					<Input id="camount" bind:value={cAmount} inputmode="decimal" placeholder="0.00" class="font-mono" />
				</Field.Field>
			</div>
			<Field.Field>
				<Field.Label for="ccode">Payee code</Field.Label>
				<Input id="ccode" bind:value={cCode} placeholder="ozky…" class="font-mono text-sm" />
			</Field.Field>
		</Field.Group>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (contribDialog = false)}>Cancel</Button>
			<Button onclick={doContribute}>Contribute</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Release confirm -->
<AlertDialog.Root open={confirmRelease !== null} onOpenChange={(o) => { if (!o) confirmRelease = null; }}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Release this escrow?</AlertDialog.Title>
			<AlertDialog.Description>
				This proves the raised total meets the release rule and mints one shielded note of the total to you.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={doRelease}>Release</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<!-- Refund confirm -->
<AlertDialog.Root open={confirmRefund !== null} onOpenChange={(o) => { if (!o) confirmRefund = null; }}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Refund your contribution?</AlertDialog.Title>
			<AlertDialog.Description>
				The escrow missed its target and the deadline has passed. This mints your contribution back to you.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action onclick={doRefund}>Refund</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={proving} title={provingTitle} />
