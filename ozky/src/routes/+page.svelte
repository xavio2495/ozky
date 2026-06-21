<script lang="ts">
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import BalanceStat from '$lib/components/shared/BalanceStat.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import * as Empty from '$lib/components/ui/empty';
	import * as Alert from '$lib/components/ui/alert';
	import PlugZapIcon from '@lucide/svelte/icons/plug-zap';
	import { wallet } from '$lib/wallet.svelte';
	import { truncate, prettyAmount } from '$lib/format';
	import { assetByCode } from '$lib/assets';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ArrowDownLeftIcon from '@lucide/svelte/icons/arrow-down-left';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import ActivityIcon from '@lucide/svelte/icons/activity';

	const actions = [
		{ href: '/send', label: 'Send', icon: ArrowUpRightIcon },
		{ href: '/receive', label: 'Receive', icon: ArrowDownLeftIcon },
		{ href: '/deposit', label: 'Deposit', icon: DownloadIcon },
		{ href: '/withdraw', label: 'Withdraw', icon: UploadIcon }
	];

	const rel = (ts: number) => {
		const s = Math.round((Date.now() - ts) / 1000);
		if (s < 60) return `${s}s ago`;
		if (s < 3600) return `${Math.floor(s / 60)}m ago`;
		return `${Math.floor(s / 3600)}h ago`;
	};
</script>

<Workspace title="Dashboard" subtitle="Your shielded balances on {wallet.network}">
	{#snippet main()}
		<div class="flex flex-col gap-7">
			<section>
				<h2 class="mb-3 text-sm font-medium text-muted-foreground">Shielded balances</h2>
				{#if wallet.loading && wallet.balances.length === 0}
					<div class="grid grid-cols-2 gap-4">
						{#each Array(4) as _}
							<Skeleton class="h-[92px] rounded-lg" />
						{/each}
					</div>
				{:else if wallet.notConfigured}
					<Alert.Root>
						<PlugZapIcon />
						<Alert.Title>Not connected to a pool</Alert.Title>
						<Alert.Description>
							Balances are unavailable until the shielded pool is configured (testnet contract
							IDs). Set <code class="font-mono text-xs">OZKY_POOL_CONTRACT</code> /
							<code class="font-mono text-xs">OZKY_POLICY_CONTRACT</code> for this build.
						</Alert.Description>
					</Alert.Root>
				{:else}
					<div class="grid grid-cols-2 gap-4">
						{#each wallet.balances as b (b.code)}
							<BalanceStat code={b.code} display={b.display} />
						{/each}
					</div>
				{/if}
			</section>

			<section>
				<div class="mb-3 flex items-center justify-between">
					<h2 class="text-sm font-medium text-muted-foreground">Public balance (unshielded)</h2>
					<span class="text-xs text-muted-foreground">on {wallet.activeAccount?.address ? truncate(wallet.activeAccount.address, 5, 5) : '…'}</span>
				</div>
				{#if wallet.publicBalances.length === 0}
					<div class="rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
						No public funds yet. Fund this account from a faucet or exchange, then Deposit to shield.
					</div>
				{:else}
					<div class="flex flex-col divide-y divide-border overflow-hidden rounded-lg border border-border bg-card/40">
						{#each wallet.publicBalances as p (p.code + (p.issuer ?? ''))}
							<div class="flex items-center justify-between px-4 py-3">
								<span class="flex items-center gap-2 text-sm font-medium">
									<span class="size-2 rounded-full" style="background:{assetByCode(p.code)?.accent ?? 'var(--muted-foreground)'}"></span>
									{p.code}
								</span>
								<span class="font-mono text-sm tabular-nums">{prettyAmount(p.balance)}</span>
							</div>
						{/each}
					</div>
				{/if}
			</section>

			<section>
				<h2 class="mb-3 text-sm font-medium text-muted-foreground">Quick actions</h2>
				<div class="grid grid-cols-4 gap-3">
					{#each actions as a (a.href)}
						<Button href={a.href} variant="outline" class="h-auto flex-col gap-2 py-5">
							<a.icon class="size-5 text-primary" />
							{a.label}
						</Button>
					{/each}
				</div>
			</section>
		</div>
	{/snippet}

	{#snippet aside()}
		<div class="flex h-full flex-col">
			<h2 class="mb-3 flex items-center gap-2 text-sm font-medium text-muted-foreground">
				<ActivityIcon class="size-4" /> Activity
			</h2>
			{#if wallet.activity.length === 0}
				<Empty.Root class="rounded-lg border border-dashed py-10">
					<Empty.Content>
						<Empty.Description>No activity yet this session.</Empty.Description>
					</Empty.Content>
				</Empty.Root>
			{:else}
				<ul class="flex flex-col gap-2">
					{#each wallet.activity as a (a.id)}
						<li class="rounded-lg border border-border bg-card/50 p-3">
							<div class="flex items-center justify-between">
								<span class="text-sm font-medium capitalize">{a.label}</span>
								<span class="text-xs text-muted-foreground">{rel(a.ts)}</span>
							</div>
							{#if a.detail}<p class="mt-0.5 text-xs text-muted-foreground">{a.detail}</p>{/if}
							{#if a.hash}
								<p class="mt-1 font-mono text-xs text-primary">{truncate(a.hash, 10, 8)}</p>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
		</div>
	{/snippet}
</Workspace>
