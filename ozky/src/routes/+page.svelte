<script lang="ts">
	import { fly } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import PrivacyCarousel from '$lib/components/ui-kit/PrivacyCarousel.svelte';
	import PendingCarousel from '$lib/components/ui-kit/PendingCarousel.svelte';
	import AccountAvatar from '$lib/components/nav/AccountAvatar.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import * as Empty from '$lib/components/ui/empty';
	import { wallet } from '$lib/wallet.svelte';
	import { truncate, prettyAmount, toNumber } from '$lib/format';
	import { assetByCode } from '$lib/assets';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ArrowDownLeftIcon from '@lucide/svelte/icons/arrow-down-left';
	import ArrowLeftRightIcon from '@lucide/svelte/icons/arrow-left-right';
	import ScaleIcon from '@lucide/svelte/icons/scale';

	const actions = [
		{ href: '/send', label: 'Send', icon: ArrowUpRightIcon },
		{ href: '/receive', label: 'Receive', icon: ArrowDownLeftIcon },
		{ href: '/swap', label: 'Swap', icon: ArrowLeftRightIcon },
		{ href: '/auditor', label: 'Audit', icon: ScaleIcon }
	];

	const rel = (ts: number) => {
		const s = Math.round((Date.now() - ts) / 1000);
		if (s < 60) return `${s}s ago`;
		if (s < 3600) return `${Math.floor(s / 60)}m ago`;
		return `${Math.floor(s / 3600)}h ago`;
	};
	const fmtUsd = (n: number) =>
		`$${n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
	const fmtNum = (n: number) => n.toLocaleString('en-US', { maximumFractionDigits: 2 });
	const fmtDate = (unix: number) =>
		new Date(unix * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric' });

	// One honest row per asset: shielded + public + total, valued in USD when priced.
	type AssetRow = {
		code: string;
		shielded: number;
		pub: number;
		total: number;
		usd: number;
		coverage: number;
		change: number;
		hasPrice: boolean;
	};
	const assetRows = $derived.by<AssetRow[]>(() => {
		const map = new Map<string, AssetRow>();
		const get = (code: string) =>
			map.get(code) ??
			map.set(code, {
				code,
				shielded: 0,
				pub: 0,
				total: 0,
				usd: 0,
				coverage: 0,
				change: 0,
				hasPrice: false
			}).get(code)!;
		for (const b of wallet.balances) get(b.code).shielded += toNumber(b.display);
		for (const p of wallet.publicBalances) get(p.code).pub += Number(p.balance) || 0;
		const out = [...map.values()].filter((r) => r.shielded > 0 || r.pub > 0);
		for (const r of out) {
			const spot = wallet.prices.find((s) => s.code === r.code);
			r.total = r.shielded + r.pub;
			r.hasPrice = !!spot;
			r.usd = r.total * (spot?.usd ?? 0);
			r.change = spot?.change_24h ?? 0;
			r.coverage = r.total > 0 ? r.shielded / r.total : 0;
		}
		return out.sort((a, b) => b.usd - a.usd || b.total - a.total);
	});

	const pricesReady = $derived(wallet.prices.length > 0);
	const totalUsd = $derived(assetRows.reduce((s, r) => s + r.usd, 0));
	const shieldedUsd = $derived(
		assetRows.reduce((s, r) => s + r.shielded * wallet.priceOf(r.code), 0)
	);
	const publicUsd = $derived(totalUsd - shieldedUsd);
	const coverageOverall = $derived(totalUsd > 0 ? shieldedUsd / totalUsd : 0);
	const change24 = $derived(
		totalUsd > 0 ? assetRows.reduce((s, r) => s + r.change * r.usd, 0) / totalUsd : 0
	);

	type Row = { label: string; href: string };
	const pendingRows = $derived.by<Row[]>(() => {
		const rows: Row[] = [];
		for (const p of wallet.payrolls.filter((p) => p.due))
			rows.push({ label: `Payroll "${p.label}" due`, href: '/payroll' });
		for (const s of wallet.subscriptions.filter((s) => s.due))
			rows.push({ label: `Subscription "${s.label}" due`, href: '/subscriptions' });
		for (const e of wallet.escrows.filter((e) => e.releasable || e.refundable))
			rows.push({
				label: `Escrow #${e.id} ${e.releasable ? 'releasable' : 'refundable'}`,
				href: '/escrow'
			});
		for (const c of wallet.channels.filter((c) => c.closeable || c.reclaimable))
			rows.push({
				label: `Channel #${c.id} ${c.closeable ? 'closeable' : 'reclaimable'}`,
				href: '/subscriptions'
			});
		for (const pb of wallet.publicBalances)
			if ((Number(pb.balance) || 0) > 0)
				rows.push({ label: `Deposit ${prettyAmount(pb.balance)} ${pb.code}`, href: '/deposit' });
		return rows;
	});
	let activityTab = $state<'recent' | 'upcoming'>('upcoming');
	const recentActivity = $derived(wallet.activity.slice(0, 6));
	type Up = { label: string; ts: number; href: string };
	const upcoming = $derived.by<Up[]>(() => {
		const list: Up[] = [];
		for (const p of wallet.payrolls)
			if (p.next_run_unix) list.push({ label: `Payroll "${p.label}"`, ts: p.next_run_unix, href: '/payroll' });
		for (const s of wallet.subscriptions)
			if (s.next_run_unix)
				list.push({ label: `Subscription "${s.label}"`, ts: s.next_run_unix, href: '/subscriptions' });
		return list.sort((a, b) => a.ts - b.ts).slice(0, 6);
	});

	const skeleton = $derived(wallet.loading && wallet.balances.length === 0);
</script>

<div class="dash">
	{#if skeleton}
		<div class="bento">
			<Skeleton class="a-total rounded-3xl" />
			<Skeleton class="a-ring rounded-3xl" />
			<Skeleton class="a-pending rounded-3xl" />
			<Skeleton class="a-assets rounded-3xl" />
			<Skeleton class="a-activity rounded-3xl" />
		</div>
	{:else}
		<div class="bento" in:fly={{ y: 12, duration: 340, easing: cubicOut }}>
			<!-- Total balance (shielded + public — the honest total) -->
			<section class="card hero transparent a-total">
				<div class="row-between">
					<span class="muted-label">Total balance</span>
					<span class="chip"><span class="dot"></span>{wallet.network}</span>
				</div>
				{#if pricesReady && totalUsd > 0}
					<div class="total">{fmtUsd(totalUsd)}</div>
					<div class="split-line">
						<span class="change" class:neg={change24 < 0}>
							{change24 >= 0 ? '▲' : '▼'}{Math.abs(change24).toFixed(2)}% · 24h
						</span>
						<span class="dotsep">·</span>
						{fmtUsd(shieldedUsd)} shielded · {fmtUsd(publicUsd)} public
					</div>
				{:else if assetRows.length > 0}
					<div class="total small">{assetRows.length} asset{assetRows.length === 1 ? '' : 's'}</div>
					<div class="muted-label">USD prices unavailable</div>
				{:else}
					<div class="total small">No funds yet</div>
					<div class="muted-label">Deposit or receive to get started</div>
				{/if}
				<div class="quick">
					{#each actions as a (a.href)}
						<Button href={a.href} variant="outline" class="h-auto flex-col gap-1.5 py-2.5">
							<a.icon class="size-4 text-primary" />
							<span class="text-xs">{a.label}</span>
						</Button>
					{/each}
				</div>
			</section>

			<!-- Privacy visualizations (carousel: donut · bars · coverage) -->
			<section class="card a-ring">
				<div class="row-between mb-2">
					<h3 class="card-title">Privacy</h3>
					<a class="jump" href="/wallet" aria-label="Open Wallet"><ArrowUpRightIcon class="size-4" /></a>
				</div>
				<div class="carousel-host">
					<PrivacyCarousel {shieldedUsd} {publicUsd} {coverageOverall} assets={assetRows} />
				</div>
			</section>

			<!-- Pending + brand promos (carousel) — the one emphasis card -->
			<section class="card  a-pending">
				<PendingCarousel pending={pendingRows} />
			</section>

			<!-- Assets: shielded + public + total + coverage, one row per asset -->
			<section class="card a-assets">
				<div class="row-between mb-2">
					<h3 class="card-title">Assets</h3>
					<a class="view-all" href="/wallet">View all</a>
				</div>
				{#if assetRows.length === 0}
					<p class="muted">No funds yet — Deposit or receive to get started.</p>
				{:else}
					<div class="alist">
						{#each assetRows as r (r.code)}
							{@const meta = assetByCode(r.code)}
							<div class="arow">
								<span
									class="glyph"
									style="color:{meta?.accent ?? 'var(--primary)'}; background:color-mix(in oklch, {meta?.accent ??
										'var(--primary)'} 16%, var(--card));"
								>
									{r.code.slice(0, 2)}
								</span>
								<div class="aid">
									<div class="truncate text-sm font-medium">{r.code}</div>
									<div class="truncate text-xs text-muted-foreground">{meta?.name ?? r.code}</div>
								</div>
								<div class="asplit">
									<div><span class="sval">{fmtNum(r.shielded)}</span> <span class="slbl">shielded</span></div>
									<div><span class="sval">{fmtNum(r.pub)}</span> <span class="slbl">public</span></div>
								</div>
								<div class="atotal">
									<div class="usd">{r.hasPrice ? fmtUsd(r.usd) : '—'}</div>
									<div class="cov-mini" title="{Math.round(r.coverage * 100)}% shielded">
										<div class="cov-mini-fill" style="width:{Math.round(r.coverage * 100)}%"></div>
									</div>
								</div>
							</div>
						{/each}
					</div>
					{#if wallet.notConfigured}
						<p class="note">Shielded balances unavailable — pool not connected.</p>
					{/if}
				{/if}
			</section>

			<!-- Activity / Upcoming (tabbed) -->
			<section class="card a-activity">
				<div class="row-between mb-2">
					<h3 class="card-title">Activity</h3>
					<div class="seg">
						<button
							class="seg-btn"
							class:active={activityTab === 'recent'}
							onclick={() => (activityTab = 'recent')}>Recent</button
						>
						<button
							class="seg-btn"
							class:active={activityTab === 'upcoming'}
							onclick={() => (activityTab = 'upcoming')}>Upcoming</button
						>
					</div>
				</div>
				{#if activityTab === 'recent'}
					{#if wallet.activity.length === 0}
						<Empty.Root class="rounded-2xl border border-dashed py-6">
							<Empty.Content>
								<Empty.Description>No activity yet this session.</Empty.Description>
							</Empty.Content>
						</Empty.Root>
					{:else}
						<ul class="act-list">
							{#each recentActivity as a (a.id)}
								<li class="act-row">
									<AccountAvatar seed={a.hash ?? a.kind} size={26} />
									<span class="truncate text-sm font-medium capitalize">{a.label}</span>
									{#if a.hash}<span class="truncate font-mono text-xs text-primary"
											>{truncate(a.hash, 6, 4)}</span
										>{/if}
									<span class="ml-auto shrink-0 text-xs text-muted-foreground">{rel(a.ts)}</span>
								</li>
							{/each}
						</ul>
						{#if wallet.activity.length > recentActivity.length}
							<a class="view-all mt-1" href="/transactions">View all</a>
						{/if}
					{/if}
				{:else if upcoming.length === 0}
					<Empty.Root class="rounded-2xl border border-dashed py-6">
						<Empty.Content>
							<Empty.Description>No upcoming scheduled runs.</Empty.Description>
						</Empty.Content>
					</Empty.Root>
				{:else}
					<ul class="act-list">
						{#each upcoming as u (u.label + u.ts)}
							<li>
								<a href={u.href} class="act-row">
									<span class="up-dot"></span>
									<span class="truncate text-sm font-medium">{u.label}</span>
									<span class="ml-auto shrink-0 text-xs text-muted-foreground">{fmtDate(u.ts)}</span>
								</a>
							</li>
						{/each}
					</ul>
				{/if}
			</section>
		</div>
	{/if}
</div>

<style>
	.dash {
		height: 100%;
		overflow: hidden;
		padding: 16px 24px 20px;
	}
	.bento {
		display: grid;
		height: 100%;
		gap: 14px;
		grid-template-columns: repeat(12, minmax(0, 1fr));
		grid-template-rows: auto minmax(0, 1fr);
		grid-template-areas:
			'total total total total total ring ring ring ring pend pend pend'
			'assets assets assets assets assets assets assets activity activity activity activity activity';
	}
	:global(.a-total) {
		grid-area: total;
	}
	:global(.a-ring) {
		grid-area: ring;
	}
	:global(.a-pending) {
		grid-area: pend;
	}
	:global(.a-assets) {
		grid-area: assets;
	}
	:global(.a-activity) {
		grid-area: activity;
	}
	/* Below the enforced min window width, fall back to a simple scroll. */
	@media (max-width: 899px) {
		.dash {
			overflow-y: auto;
		}
		.bento {
			height: auto;
			grid-template-columns: 1fr;
			grid-template-areas: 'total' 'ring' 'pend' 'assets' 'activity';
		}
	}

	.card {
		display: flex;
		flex-direction: column;
		min-height: 0;
		padding: 16px 18px;
		border: 1px solid var(--border);
		border-radius: var(--radius-3xl);
		background: var(--card);
		/* backdrop-filter: blur(16px);
		-webkit-backdrop-filter: blur(16px);
		box-shadow:
			0 1px 0 0 color-mix(in oklch, white 4%, transparent) inset,
			0 8px 24px -12px rgb(0 0 0 / 0.6); */
	}
	.card.transparent {
		background: transparent;
		backdrop-filter: none;
		-webkit-backdrop-filter: none;
		border-color: transparent;
		box-shadow: none;
		padding-left: 0;
		padding-right: 0;
	}
	.a-assets,
	.a-activity {
		overflow: hidden;
	}
	.carousel-host {
		flex: 1;
		min-height: 0;
		display: flex;
		align-items: center;
		padding: 0 8px;
	}
	.row-between {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}
	.card-title {
		font-family: var(--font-heading);
		font-size: 0.9375rem;
		font-weight: 600;
	}
	.muted {
		font-size: 0.875rem;
		color: var(--muted-foreground);
	}
	.muted-label {
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
	.note {
		margin-top: 8px;
		font-size: 0.6875rem;
		color: var(--muted-foreground);
	}
	.jump {
		display: grid;
		place-items: center;
		width: 28px;
		height: 28px;
		flex-shrink: 0;
		border: 1px solid var(--border);
		border-radius: 9999px;
		color: var(--muted-foreground);
		transition: color 0.15s ease, border-color 0.15s ease;
	}
	.jump:hover {
		color: var(--primary);
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
	.view-all {
		font-size: 0.75rem;
		color: var(--primary);
	}
	.view-all:hover {
		text-decoration: underline;
	}
	/* Total card */
	.hero {
		justify-content: space-between;
	}
	.chip {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		padding: 4px 10px;
		border: 1px solid var(--border);
		border-radius: 9999px;
		background: color-mix(in oklch, var(--card) 50%, transparent);
		font-size: 0.6875rem;
		text-transform: capitalize;
	}
	.dot {
		width: 6px;
		height: 6px;
		border-radius: 9999px;
		background: var(--primary);
		box-shadow: 0 0 8px var(--primary);
	}
	.total {
		font-family: var(--font-heading);
		font-weight: 600;
		letter-spacing: -0.02em;
		font-size: clamp(2.25rem, 3.4vw, 3rem);
		line-height: 1.04;
		font-variant-numeric: tabular-nums;
		margin-top: 6px;
	}
	.total.small {
		font-size: 1.6rem;
	}
	.split-line {
		margin-top: 2px;
		font-size: 0.75rem;
		color: var(--muted-foreground);
		font-variant-numeric: tabular-nums;
	}
	.change {
		color: var(--primary);
	}
	.change.neg {
		color: var(--destructive);
	}
	.dotsep {
		margin: 0 2px;
	}
	.quick {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		gap: 8px;
		margin-top: 14px;
	}
	/* Assets table */
	.alist {
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow: hidden;
	}
	.arow {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 9px 2px;
		border-bottom: 1px solid var(--border);
	}
	.arow:last-child {
		border-bottom: none;
	}
	.glyph {
		display: grid;
		place-items: center;
		width: 34px;
		height: 34px;
		flex-shrink: 0;
		border-radius: 9999px;
		font-size: 0.6875rem;
		font-weight: 700;
	}
	.aid {
		min-width: 0;
		flex: 1;
	}
	.asplit {
		flex-shrink: 0;
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	.sval {
		font-family: var(--font-mono, monospace);
		font-size: 0.75rem;
	}
	.slbl {
		font-size: 0.625rem;
		color: var(--muted-foreground);
	}
	.atotal {
		flex-shrink: 0;
		width: 92px;
		text-align: right;
	}
	.usd {
		font-family: var(--font-mono, monospace);
		font-size: 0.875rem;
		font-variant-numeric: tabular-nums;
	}
	.cov-mini {
		margin-top: 4px;
		height: 5px;
		border-radius: 9999px;
		overflow: hidden;
		background: repeating-linear-gradient(
			45deg,
			var(--muted),
			var(--muted) 4px,
			color-mix(in oklch, var(--primary) 9%, transparent) 4px,
			color-mix(in oklch, var(--primary) 9%, transparent) 8px
		);
	}
	.cov-mini-fill {
		height: 100%;
		border-radius: 9999px;
		background: var(--primary);
		transition: width 0.5s cubic-bezier(0.22, 1, 0.36, 1);
	}
	/* Segmented tabs */
	.seg {
		display: inline-flex;
		gap: 2px;
		padding: 3px;
		border: 1px solid var(--border);
		border-radius: 9999px;
		background: color-mix(in oklch, var(--card) 50%, transparent);
	}
	.seg-btn {
		padding: 3px 11px;
		border-radius: 9999px;
		font-size: 0.6875rem;
		font-weight: 500;
		color: var(--muted-foreground);
		transition: color 0.15s ease, background 0.15s ease;
	}
	.seg-btn:hover {
		color: var(--foreground);
	}
	.seg-btn.active {
		background: var(--primary);
		color: var(--primary-foreground);
	}
	/* Activity */
	.act-list {
		display: flex;
		flex-direction: column;
		gap: 1px;
		min-height: 0;
		overflow: hidden;
	}
	.act-row {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 6px 4px;
	}
	.up-dot {
		width: 26px;
		height: 26px;
		flex-shrink: 0;
		border-radius: 9999px;
		background: color-mix(in oklch, var(--primary) 16%, var(--card));
		box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--primary) 30%, var(--border));
	}
</style>
