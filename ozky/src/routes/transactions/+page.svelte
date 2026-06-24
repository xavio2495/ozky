<script lang="ts">
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import * as Empty from '$lib/components/ui/empty';
	import * as Alert from '$lib/components/ui/alert';
	import * as Tabs from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import { wallet } from '$lib/wallet.svelte';
	import { api, errMessage, type PublicTx } from '$lib/api';
	import { truncate, prettyAmount } from '$lib/format';
	import { toast } from 'svelte-sonner';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ArrowDownLeftIcon from '@lucide/svelte/icons/arrow-down-left';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import ScaleIcon from '@lucide/svelte/icons/scale';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import ReceiptIcon from '@lucide/svelte/icons/receipt';
	import SplitIcon from '@lucide/svelte/icons/split';
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import RepeatIcon from '@lucide/svelte/icons/repeat';
	import Repeat2Icon from '@lucide/svelte/icons/repeat-2';
	import HandCoinsIcon from '@lucide/svelte/icons/hand-coins';
	import ArrowLeftRightIcon from '@lucide/svelte/icons/arrow-left-right';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import InfoIcon from '@lucide/svelte/icons/info';

	const icons = {
		deposit: DownloadIcon,
		send: ArrowUpRightIcon,
		split: SplitIcon,
		payroll: CalendarClockIcon,
		subscription: RepeatIcon,
		escrow: HandCoinsIcon,
		channel: Repeat2Icon,
		withdraw: UploadIcon,
		swap: ArrowLeftRightIcon,
		enroll: ShieldCheckIcon,
		disclose: ScaleIcon
	} as const;

	const fmtTime = (ts: number) =>
		new Date(ts).toLocaleString('en-US', {
			month: 'short',
			day: 'numeric',
			hour: '2-digit',
			minute: '2-digit'
		});

	const explorer = (hash: string) => `https://stellar.expert/explorer/testnet/tx/${hash}`;

	// Public history is fetched lazily on first view of the Public tab.
	let publicTxs = $state<PublicTx[]>([]);
	let publicLoading = $state(false);
	let publicLoaded = $state(false);

	async function loadPublic() {
		if (publicLoaded || publicLoading) return;
		publicLoading = true;
		try {
			publicTxs = await api.publicHistory();
			publicLoaded = true;
		} catch (e) {
			toast.error('Could not load public history', { description: errMessage(e) });
		} finally {
			publicLoading = false;
		}
	}

	function onTab(value: string | undefined) {
		if (value === 'public') void loadPublic();
	}
</script>

<Workspace title="Transactions" subtitle="Activity for {wallet.activeAccount?.label ?? 'this account'}">
	{#snippet main()}
		<Tabs.Root value="shielded" onValueChange={onTab} class="flex flex-col gap-4">
			<Tabs.List>
				<Tabs.Trigger value="shielded">Shielded</Tabs.Trigger>
				<Tabs.Trigger value="public">Public</Tabs.Trigger>
			</Tabs.List>

			<!-- Shielded: the wallet's durable pool activity -->
			<Tabs.Content value="shielded">
				{#if wallet.activity.length === 0}
					<Empty.Root class="rounded-xl border border-dashed py-16">
						<Empty.Header>
							<Empty.Media variant="icon"><ReceiptIcon /></Empty.Media>
							<Empty.Title>No shielded activity yet</Empty.Title>
							<Empty.Description>
								Deposits, sends, splits, and other private flows you make will appear here.
							</Empty.Description>
						</Empty.Header>
					</Empty.Root>
				{:else}
					<div class="flex flex-col gap-2">
						{#each wallet.activity as a (a.id)}
							{@const Icon = icons[a.kind]}
							<div class="row">
								<span class="grid size-9 place-items-center rounded-lg bg-primary/12 text-primary">
									<Icon class="size-4" />
								</span>
								<div class="min-w-0 flex-1">
									<div class="flex items-center gap-2">
										<span class="truncate text-sm font-medium">{a.label}</span>
										<Badge variant="outline" class="capitalize">{a.kind}</Badge>
									</div>
									{#if a.detail}<p class="truncate text-xs text-muted-foreground">{a.detail}</p>{/if}
								</div>
								<div class="flex flex-col items-end gap-1">
									<span class="text-xs text-muted-foreground">{fmtTime(a.ts)}</span>
									{#if a.hash}
										<div class="flex items-center gap-1">
											<a href={explorer(a.hash)} target="_blank" rel="noreferrer" class="font-mono text-xs text-primary hover:underline">
												{truncate(a.hash, 6, 6)}
											</a>
											<CopyButton text={a.hash} size="icon" variant="ghost" />
										</div>
									{/if}
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</Tabs.Content>

			<!-- Public: the funding account's classic Stellar payments -->
			<Tabs.Content value="public">
				{#if publicLoading}
					<p class="py-10 text-center text-sm text-muted-foreground">Loading public history…</p>
				{:else if publicTxs.length === 0}
					<Empty.Root class="rounded-xl border border-dashed py-16">
						<Empty.Header>
							<Empty.Media variant="icon"><GlobeIcon /></Empty.Media>
							<Empty.Title>No public payments</Empty.Title>
							<Empty.Description>
								Classic payments to or from your public funding address will appear here.
							</Empty.Description>
						</Empty.Header>
					</Empty.Root>
				{:else}
					<div class="flex flex-col gap-2">
						{#each publicTxs as p (p.hash + p.ts)}
							{@const received = p.direction === 'received'}
							<div class="row">
								<span
									class="grid size-9 place-items-center rounded-lg"
									style="background: color-mix(in oklch, {received ? 'oklch(0.7 0.15 155)' : 'var(--primary)'} 12%, transparent); color: {received ? 'oklch(0.7 0.15 155)' : 'var(--primary)'}"
								>
									{#if received}<ArrowDownLeftIcon class="size-4" />{:else}<ArrowUpRightIcon class="size-4" />{/if}
								</span>
								<div class="min-w-0 flex-1">
									<div class="flex items-center gap-2">
										<span class="truncate text-sm font-medium">
											{received ? 'Received' : 'Sent'} {prettyAmount(p.amount)} {p.asset}
										</span>
										<Badge variant="outline" class="capitalize">{p.kind.replace('_', ' ')}</Badge>
									</div>
									{#if p.counterparty}
										<p class="truncate font-mono text-xs text-muted-foreground">
											{received ? 'from' : 'to'} {truncate(p.counterparty, 6, 6)}
										</p>
									{/if}
								</div>
								<div class="flex flex-col items-end gap-1">
									<span class="text-xs text-muted-foreground">{fmtTime(p.ts)}</span>
									<div class="flex items-center gap-1">
										<a href={explorer(p.hash)} target="_blank" rel="noreferrer" class="font-mono text-xs text-primary hover:underline">
											{truncate(p.hash, 6, 6)}
										</a>
										<CopyButton text={p.hash} size="icon" variant="ghost" />
									</div>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</Tabs.Content>
		</Tabs.Root>
	{/snippet}

	{#snippet aside()}
		<Alert.Root>
			<InfoIcon />
			<Alert.Title>Shielded vs public</Alert.Title>
			<Alert.Description>
				<ul class="mt-1 flex list-disc flex-col gap-1 pl-4 text-xs">
					<li><strong>Shielded</strong> — your private pool actions (deposit / send / withdraw / split / escrow / channel), saved encrypted on this device.</li>
					<li><strong>Public</strong> — classic payments on your funding address, read live from the network.</li>
					<li>Incoming private receives show in your balance; itemizing them here is a follow-up.</li>
				</ul>
			</Alert.Description>
		</Alert.Root>
	{/snippet}
</Workspace>

<style>
	.row {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 12px 14px;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: color-mix(in oklch, var(--card) 50%, transparent);
		transition: border-color 0.15s ease;
	}
	.row:hover {
		border-color: color-mix(in oklch, var(--primary) 30%, var(--border));
	}
</style>
