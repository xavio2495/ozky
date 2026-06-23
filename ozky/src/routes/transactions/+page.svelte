<script lang="ts">
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import * as Empty from '$lib/components/ui/empty';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import { wallet } from '$lib/wallet.svelte';
	import { truncate } from '$lib/format';
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
	import HandCoinsIcon from '@lucide/svelte/icons/hand-coins';
	import InfoIcon from '@lucide/svelte/icons/info';

	const icons = {
		deposit: DownloadIcon,
		send: ArrowUpRightIcon,
		split: SplitIcon,
		payroll: CalendarClockIcon,
		subscription: RepeatIcon,
		escrow: HandCoinsIcon,
		withdraw: UploadIcon,
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
</script>

<Workspace title="Transactions" subtitle="Activity for {wallet.activeAccount?.label ?? 'this account'}">
	{#snippet main()}
		{#if wallet.activity.length === 0}
			<Empty.Root class="rounded-xl border border-dashed py-16">
				<Empty.Header>
					<Empty.Media variant="icon"><ReceiptIcon /></Empty.Media>
					<Empty.Title>No transactions yet</Empty.Title>
					<Empty.Description>
						Deposits, sends, and withdrawals you make will appear here.
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
	{/snippet}

	{#snippet aside()}
		<Alert.Root>
			<InfoIcon />
			<Alert.Title>Session history</Alert.Title>
			<Alert.Description>
				This lists actions from the current session. A full persistent history (rebuilt from
				on-chain note scans) arrives with the indexer.
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
