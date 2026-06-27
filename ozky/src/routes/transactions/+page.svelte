<script lang="ts">
	import { onMount } from 'svelte';
	import { fly } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import * as Table from '$lib/components/ui/table';
	import * as Select from '$lib/components/ui/select';
	import * as Empty from '$lib/components/ui/empty';
	import * as Alert from '$lib/components/ui/alert';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import AccountAvatar from '$lib/components/nav/AccountAvatar.svelte';
	import { wallet } from '$lib/wallet.svelte';
	import { api, errMessage, type PublicTx } from '$lib/api';
	import { truncate, prettyAmount } from '$lib/format';
	import { assetByCode } from '$lib/assets';
	import { toast } from 'svelte-sonner';
	import type { Component } from 'svelte';
	import SearchIcon from '@lucide/svelte/icons/search';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import XIcon from '@lucide/svelte/icons/x';
	import ReceiptIcon from '@lucide/svelte/icons/receipt';
	import InfoIcon from '@lucide/svelte/icons/info';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ArrowDownLeftIcon from '@lucide/svelte/icons/arrow-down-left';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import ScaleIcon from '@lucide/svelte/icons/scale';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import SplitIcon from '@lucide/svelte/icons/split';
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import RepeatIcon from '@lucide/svelte/icons/repeat';
	import Repeat2Icon from '@lucide/svelte/icons/repeat-2';
	import HandCoinsIcon from '@lucide/svelte/icons/hand-coins';
	import ArrowLeftRightIcon from '@lucide/svelte/icons/arrow-left-right';

	const kindIcons: Record<string, Component> = {
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
		disclose: ScaleIcon,
		payment: ArrowLeftRightIcon,
		create_account: DownloadIcon
	};

	const fmtTime = (ts: number) =>
		new Date(ts).toLocaleString('en-US', {
			month: 'short',
			day: 'numeric',
			hour: '2-digit',
			minute: '2-digit'
		});
	const rel = (ts: number) => {
		const s = Math.round((Date.now() - ts) / 1000);
		if (s < 60) return `${s}s ago`;
		if (s < 3600) return `${Math.floor(s / 60)}m ago`;
		if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
		return `${Math.floor(s / 86400)}d ago`;
	};
	const explorer = (hash: string) => `https://stellar.expert/explorer/${wallet.network}/tx/${hash}`;
	// In the Tauri webview a plain <a target="_blank"> is a no-op — route external links through
	// the opener plugin so they open in the user's real browser.
	async function openTx(hash: string) {
		try {
			await openUrl(explorer(hash));
		} catch (e) {
			toast.error('Could not open explorer', { description: errMessage(e) });
		}
	}

	type TxRow = {
		uid: string;
		layer: 'shielded' | 'public';
		kind: string;
		icon: Component;
		label: string;
		detail?: string;
		asset?: string;
		amount?: string;
		direction: 'in' | 'out' | null;
		counterparty?: string;
		hash?: string;
		ts: number;
	};

	// Public history is pulled lazily on mount (shielded comes from the live store).
	let publicTxs = $state<PublicTx[]>([]);
	let publicLoading = $state(false);
	let publicError = $state(false);

	async function loadPublic() {
		publicLoading = true;
		publicError = false;
		try {
			publicTxs = await api.publicHistory();
		} catch (e) {
			publicError = true;
			toast.error('Could not load public history', { description: errMessage(e) });
		} finally {
			publicLoading = false;
		}
	}
	onMount(loadPublic);

	const inKinds = new Set(['deposit', 'enroll']);
	const outKinds = new Set(['send', 'split', 'payroll', 'subscription', 'withdraw']);

	const rows = $derived.by<TxRow[]>(() => {
		const out: TxRow[] = [];
		for (const a of wallet.activity) {
			out.push({
				uid: `s:${a.id}`,
				layer: 'shielded',
				kind: a.kind,
				icon: kindIcons[a.kind] ?? ReceiptIcon,
				label: a.label,
				detail: a.detail,
				direction: inKinds.has(a.kind) ? 'in' : outKinds.has(a.kind) ? 'out' : null,
				hash: a.hash,
				ts: a.ts
			});
		}
		for (const p of publicTxs) {
			const received = p.direction === 'received';
			out.push({
				uid: `p:${p.hash}:${p.ts}`,
				layer: 'public',
				kind: p.kind,
				icon: received ? ArrowDownLeftIcon : ArrowUpRightIcon,
				label: `${received ? 'Received' : 'Sent'} ${prettyAmount(p.amount)} ${p.asset}`,
				detail: p.counterparty
					? `${received ? 'from' : 'to'} ${truncate(p.counterparty, 6, 6)}`
					: undefined,
				asset: p.asset,
				amount: p.amount,
				direction: received ? 'in' : 'out',
				counterparty: p.counterparty,
				hash: p.hash,
				ts: p.ts
			});
		}
		return out.sort((a, b) => b.ts - a.ts);
	});

	// Filters
	let layer = $state<'all' | 'shielded' | 'public'>('all');
	let direction = $state<'all' | 'in' | 'out'>('all');
	let search = $state('');

	const filtered = $derived(
		rows.filter((r) => {
			if (layer !== 'all' && r.layer !== layer) return false;
			if (direction !== 'all' && r.direction !== direction) return false;
			if (search.trim()) {
				const q = search.toLowerCase();
				const hay =
					`${r.label} ${r.detail ?? ''} ${r.hash ?? ''} ${r.counterparty ?? ''} ${r.kind}`.toLowerCase();
				if (!hay.includes(q)) return false;
			}
			return true;
		})
	);
	const hasFilters = $derived(layer !== 'all' || direction !== 'all' || search.trim() !== '');
	function clearFilters() {
		layer = 'all';
		direction = 'all';
		search = '';
	}

	const layerLabels = { all: 'All layers', shielded: 'Shielded', public: 'Public' };
	const dirLabels = { all: 'Any direction', in: 'In', out: 'Out' };

	// Detail drawer
	let selected = $state<TxRow | null>(null);
</script>

<div class="page">
	<header class="head">
		<p class="subtitle">
			Account "{wallet.activeAccount?.label ?? 'this account'}"
		</p>
		<Button variant="outline" size="sm" onclick={loadPublic} disabled={publicLoading}>
			<RefreshCwIcon data-icon="inline-start" class={publicLoading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<!-- Filter bar -->
	<div class="filters">
		<Select.Root type="single" bind:value={layer}>
			<Select.Trigger class="w-[150px] bg-popover">{layerLabels[layer]}</Select.Trigger>
			<Select.Content class="bg-popover">
				<Select.Item value="all">All layers</Select.Item>
				<Select.Item value="shielded">Shielded</Select.Item>
				<Select.Item value="public">Public</Select.Item>
			</Select.Content>
		</Select.Root>
		<Select.Root type="single" bind:value={direction}>
			<Select.Trigger class="w-[150px] bg-popover">{dirLabels[direction]}</Select.Trigger>
			<Select.Content class="bg-popover">
				<Select.Item value="all">Any direction</Select.Item>
				<Select.Item value="in">In</Select.Item>
				<Select.Item value="out">Out</Select.Item>
			</Select.Content>
		</Select.Root>
		<div class="search">
			<SearchIcon class="size-4 text-muted-foreground" />
			<Input
				bind:value={search}
				placeholder="Search label, hash, counterparty…"
				class="border-0 bg-transparent shadow-none focus-visible:ring-0"
			/>
		</div>
		{#if hasFilters}
			<Button variant="ghost" size="sm" onclick={clearFilters}>Clear</Button>
		{/if}
	</div>

	<!-- Privacy note -->
	<Alert.Root class="border-primary/20">
		<InfoIcon />
		<Alert.Description class="text-xs">
			Shielded rows are your private local record — unlinkable on-chain. Observers and auditors see
			nothing here unless you
			<a href="/auditor" class="text-primary hover:underline">share a disclosure</a>.
		</Alert.Description>
	</Alert.Root>

	<!-- Table -->
	<div class="table-wrap">
		{#if rows.length === 0 && !publicLoading}
			<Empty.Root class="py-16">
				<Empty.Header>
					<Empty.Media variant="icon"><ReceiptIcon /></Empty.Media>
					<Empty.Title>No transactions yet</Empty.Title>
					<Empty.Description>
						Deposits, sends, swaps, and classic payments will appear here.
					</Empty.Description>
				</Empty.Header>
			</Empty.Root>
		{:else if filtered.length === 0}
			<Empty.Root class="py-16">
				<Empty.Header>
					<Empty.Media variant="icon"><SearchIcon /></Empty.Media>
					<Empty.Title>No transactions match these filters</Empty.Title>
				</Empty.Header>
				<Empty.Content>
					<Button variant="outline" size="sm" onclick={clearFilters}>Clear filters</Button>
				</Empty.Content>
			</Empty.Root>
		{:else}
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>Type</Table.Head>
						<Table.Head>Label</Table.Head>
						<Table.Head>Amount</Table.Head>
						<Table.Head>Layer</Table.Head>
						<Table.Head>Counterparty</Table.Head>
						<Table.Head class="text-right">Time</Table.Head>
						<Table.Head class="w-8"></Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each filtered as r (r.uid)}
						{@const Icon = r.icon}
						{@const meta = r.asset ? assetByCode(r.asset) : undefined}
						<Table.Row
							class="cursor-pointer"
							data-state={selected?.uid === r.uid ? 'selected' : undefined}
							onclick={() => (selected = r)}
						>
							<Table.Cell>
								<span class="type">
									<span class="ico"><Icon class="size-4" /></span>
									<span class="capitalize">{r.kind.replace('_', ' ')}</span>
								</span>
							</Table.Cell>
							<Table.Cell>
								<div class="truncate font-medium">{r.label}</div>
								{#if r.detail}<div class="truncate text-xs text-muted-foreground">{r.detail}</div>{/if}
							</Table.Cell>
							<Table.Cell class="font-mono tabular-nums">
								{#if r.amount}
									<span class:inn={r.direction === 'in'} class:out={r.direction === 'out'}>
										{r.direction === 'in' ? '+' : r.direction === 'out' ? '−' : ''}{prettyAmount(
											r.amount
										)}
									</span>
									{#if meta}<span class="text-xs text-muted-foreground"> {r.asset}</span>{/if}
								{:else}
									<span class="text-muted-foreground">—</span>
								{/if}
							</Table.Cell>
							<Table.Cell>
								<Badge variant="outline" class="gap-1.5">
									<span class="dot" class:gold={r.layer === 'shielded'}></span>
									<span class="capitalize">{r.layer}</span>
								</Badge>
							</Table.Cell>
							<Table.Cell>
								{#if r.layer === 'shielded'}
									<span class="text-sm text-muted-foreground">Private</span>
								{:else if r.counterparty}
									<span class="cparty">
										<AccountAvatar seed={r.counterparty} size={20} />
										<span class="font-mono text-xs">{truncate(r.counterparty, 4, 4)}</span>
									</span>
								{:else}
									<span class="text-muted-foreground">—</span>
								{/if}
							</Table.Cell>
							<Table.Cell class="text-right text-xs text-muted-foreground" title={fmtTime(r.ts)}>
								{rel(r.ts)}
							</Table.Cell>
							<Table.Cell>
								{#if r.hash}
									{@const h = r.hash}
									<button
										type="button"
										class="text-muted-foreground hover:text-primary"
										onclick={(e) => { e.stopPropagation(); openTx(h); }}
										aria-label="Open in explorer"
									>
										<ExternalLinkIcon class="size-4" />
									</button>
								{/if}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
			{#if publicLoading}
				<p class="py-3 text-center text-xs text-muted-foreground">Loading public history…</p>
			{:else if publicError}
				<p class="py-3 text-center text-xs text-muted-foreground">
					Public payments unavailable.
					<button class="text-primary hover:underline" onclick={loadPublic}>Retry</button>
				</p>
			{/if}
			<div class="foot">Showing {filtered.length} of {rows.length} transactions</div>
		{/if}
	</div>

	<!-- Detail panel — a custom in-page sidebar (stays below the navbar/chrome, translucent --card) -->
	{#if selected}
		{@const r = selected}
		{@const Icon = r.icon}
		<button class="detail-scrim" aria-label="Close details" onclick={() => (selected = null)}></button>
		<aside class="detail-panel" transition:fly={{ x: 420, duration: 260, easing: cubicOut }}>
			<div class="panel-head">
				<div class="drawer-head">
					<span class="ico lg"><Icon class="size-5" /></span>
					<div>
						<div class="panel-title capitalize">{r.kind.replace('_', ' ')}</div>
						<div class="panel-sub capitalize">{r.layer} · {rel(r.ts)}</div>
					</div>
				</div>
				<button class="panel-close" aria-label="Close" onclick={() => (selected = null)}><XIcon class="size-4" /></button>
			</div>
			<div class="fields">
				<div class="field"><span class="k">Title</span><span class="v">{r.label}</span></div>
				{#if r.amount}
					<div class="field">
						<span class="k">Amount</span>
						<span class="v font-mono">
							{r.direction === 'in' ? '+' : r.direction === 'out' ? '−' : ''}{prettyAmount(r.amount)}
							{r.asset ?? ''}
						</span>
					</div>
				{:else}
					<div class="field">
						<span class="k">Amount</span>
						<span class="v text-muted-foreground">Private — local record</span>
					</div>
				{/if}
				<div class="field">
					<span class="k">Layer</span>
					<span class="v">{r.layer === 'shielded' ? 'Shielded pool' : 'Public (classic Stellar)'}</span>
				</div>
				{#if r.direction}
					<div class="field">
						<span class="k">Direction</span><span class="v capitalize">{r.direction}</span>
					</div>
				{/if}
				<div class="field">
					<span class="k">Counterparty</span>
					{#if r.layer === 'shielded'}
						<span class="v text-muted-foreground">Private (hidden on-chain by design)</span>
					{:else if r.counterparty}
						<span class="v cparty">
							<AccountAvatar seed={r.counterparty} size={20} />
							<span class="font-mono text-xs">{truncate(r.counterparty, 6, 6)}</span>
							<CopyButton text={r.counterparty} size="icon" variant="ghost" />
						</span>
					{:else}<span class="v text-muted-foreground">—</span>{/if}
				</div>
				{#if r.detail}
					<div class="field"><span class="k">Detail</span><span class="v">{r.detail}</span></div>
				{/if}
				<div class="field"><span class="k">Time</span><span class="v">{fmtTime(r.ts)}</span></div>
				{#if r.hash}
					{@const h = r.hash}
					<div class="field">
						<span class="k">Hash</span>
						<span class="v cparty">
							<button type="button" class="font-mono text-xs text-primary hover:underline" onclick={() => openTx(h)}>
								{truncate(h, 6, 6)}
							</button>
							<CopyButton text={h} size="icon" variant="ghost" />
						</span>
					</div>
				{/if}
			</div>
			<div class="panel-foot">
				{#if r.hash}
					{@const h = r.hash}
					<Button variant="outline" class="flex-1" onclick={() => openTx(h)}>
						<ExternalLinkIcon data-icon="inline-start" />
						Explorer
					</Button>
				{/if}
				{#if r.layer === 'shielded'}
					<Button variant="outline" href="/auditor" class="flex-1">
						<ScaleIcon data-icon="inline-start" />
						Disclose
					</Button>
				{/if}
			</div>
		</aside>
	{/if}
</div>

<style>
	.page {
		position: relative;
		display: flex;
		flex-direction: column;
		gap: 16px;
		height: 100%;
		overflow: hidden;
		padding: 20px 32px 24px;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
	}
	.subtitle {
		font-size: 0.875rem;
		color: var(--muted-foreground);
	}
	/* In-page detail sidebar — absolute within .page, so it never covers the navbar/chrome. */
	.detail-scrim {
		position: absolute;
		inset: 0;
		z-index: 5;
		background: color-mix(in oklch, black 32%, transparent);
		backdrop-filter: blur(1px);
	}
	.detail-panel {
		position: absolute;
		top: 12px;
		right: 12px;
		bottom: 12px;
		z-index: 6;
		width: 400px;
		max-width: calc(100% - 24px);
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border);
		border-radius: var(--radius-2xl);
		background: color-mix(in oklch, var(--card) 82%, transparent);
		backdrop-filter: blur(20px);
		-webkit-backdrop-filter: blur(20px);
		box-shadow: 0 16px 48px -16px rgb(0 0 0 / 0.7);
		overflow: hidden;
	}
	.panel-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		padding: 16px 16px 12px;
		border-bottom: 1px solid var(--border);
	}
	.panel-title {
		font-family: var(--font-heading);
		font-size: 1rem;
		font-weight: 600;
	}
	.panel-sub {
		font-size: 0.75rem;
		color: var(--muted-foreground);
	}
	.panel-close {
		display: grid;
		place-items: center;
		width: 30px;
		height: 30px;
		flex-shrink: 0;
		border-radius: var(--radius-md);
		color: var(--muted-foreground);
	}
	.panel-close:hover {
		color: var(--foreground);
		background: color-mix(in oklch, var(--foreground) 8%, transparent);
	}
	.panel-foot {
		display: flex;
		gap: 8px;
		padding: 12px 16px 16px;
		border-top: 1px solid var(--border);
	}
	.filters {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
	}
	.search {
		display: flex;
		align-items: center;
		gap: 6px;
		flex: 1;
		min-width: 220px;
		padding: 0 12px;
		border: 1px solid var(--input);
		border-radius: 9999px;
		background: color-mix(in oklch, var(--card) 50%, transparent);
	}
	.table-wrap {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		border: 1px solid var(--border);
		border-radius: var(--radius-2xl);
		background: var(--card);
		/* backdrop-filter: blur(16px);
		-webkit-backdrop-filter: blur(16px); */
	}
	.type {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		font-size: 0.8125rem;
		white-space: nowrap;
	}
	.ico {
		display: grid;
		place-items: center;
		width: 30px;
		height: 30px;
		flex-shrink: 0;
		border-radius: var(--radius-md);
		background: color-mix(in oklch, var(--primary) 12%, transparent);
		color: var(--primary);
	}
	.ico.lg {
		width: 38px;
		height: 38px;
	}
	.inn {
		color: var(--primary);
	}
	.out {
		color: var(--muted-foreground);
	}
	.dot {
		width: 7px;
		height: 7px;
		border-radius: 9999px;
		background: var(--muted-foreground);
	}
	.dot.gold {
		background: var(--primary);
	}
	.cparty {
		display: inline-flex;
		align-items: center;
		gap: 6px;
	}
	.foot {
		padding: 10px 14px;
		font-size: 0.75rem;
		color: var(--muted-foreground);
		border-top: 1px solid var(--border);
	}
	.drawer-head {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.fields {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding: 4px 16px;
		overflow-y: auto;
	}
	.field {
		display: grid;
		grid-template-columns: 110px 1fr;
		gap: 12px;
		padding: 9px 0;
		border-bottom: 1px solid var(--border);
		font-size: 0.8125rem;
	}
	.field:last-child {
		border-bottom: none;
	}
	.field .k {
		color: var(--muted-foreground);
	}
	.field .v {
		min-width: 0;
		word-break: break-word;
	}
</style>
