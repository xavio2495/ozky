<script lang="ts">
	import { fly } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import { api, errMessage, type AuditResult } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import { truncate } from '$lib/format';
	import { toast } from 'svelte-sonner';
	import ScaleIcon from '@lucide/svelte/icons/scale';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import CheckCircle2Icon from '@lucide/svelte/icons/check-circle-2';

	let mode = $state<'disclose' | 'review'>('disclose');

	// ---- disclose (owner) --------------------------------------------------
	let auditor = $state('');
	let fromEpoch = $state('');
	let toEpoch = $state('');
	let sharing = $state(false);
	let pkg = $state('');

	async function share() {
		if (!auditor.trim().startsWith('G')) return toast.error('Enter the auditor’s Stellar G… address');
		const from = Number(fromEpoch) || 0;
		const to = Number(toEpoch) || from;
		if (to < from) return toast.error('“To epoch” must be ≥ “From epoch”');
		sharing = true;
		const out = await runAction('Creating disclosure', () => api.shareWithAuditor(auditor.trim(), from, to), {
			success: () => 'Disclosure created',
			refresh: false
		});
		sharing = false;
		if (out) {
			pkg = out;
			wallet.log({
				kind: 'disclose',
				label: 'Shared with auditor',
				detail: from === to ? `epoch ${from}` : `epochs ${from}–${to}`
			});
		}
	}

	// ---- review (auditor) --------------------------------------------------
	let inputPkg = $state('');
	let verifying = $state(false);
	let result = $state<AuditResult | null>(null);
	let pkgMeta = $derived.by(() => {
		try {
			const p = JSON.parse(inputPkg || '{}');
			return { owner: p.owner_stellar as string | undefined, pool: p.pool_contract as string | undefined };
		} catch {
			return { owner: undefined, pool: undefined };
		}
	});

	async function verify() {
		verifying = true;
		try {
			result = await api.auditDisclosure(inputPkg.trim());
			toast.success(`Verified ${result.notes.length} note(s)`);
		} catch (e) {
			result = null;
			toast.error('Could not verify package', { description: errMessage(e) });
		} finally {
			verifying = false;
		}
	}

	const fmtVal = (v: number) => v.toLocaleString('en-US');
</script>

<div class="hub">
	<div class="head">
		<p class="subtitle">Selective, read-only disclosure — scoped to an epoch range, no spend authority.</p>
		<ToggleGroup.Root type="single" bind:value={mode} class="modes">
			<ToggleGroup.Item value="disclose" class="text-xs">Disclose</ToggleGroup.Item>
			<ToggleGroup.Item value="review" class="text-xs">Review</ToggleGroup.Item>
		</ToggleGroup.Root>
	</div>

	{#if mode === 'disclose'}
		<div class="cols" in:fly={{ y: 12, duration: 280, easing: cubicOut }}>
			<section class="card pane">
				<div class="pane-head"><ShieldCheckIcon class="size-4 text-primary" /><h2 class="pane-title">Disclose to an auditor</h2></div>
				<p class="hint">Exports a scoped, read-only package of your notes in the chosen epoch range and records an on-chain grant to the auditor. Other epochs stay shielded.</p>
				<Field.Field>
					<Field.Label>Auditor address</Field.Label>
					<Input bind:value={auditor} placeholder="G…" class="font-mono" />
				</Field.Field>
				<div class="two">
					<Field.Field><Field.Label>From epoch</Field.Label><Input bind:value={fromEpoch} type="number" min="0" placeholder="0" /></Field.Field>
					<Field.Field><Field.Label>To epoch <span class="opt">optional</span></Field.Label><Input bind:value={toEpoch} type="number" min="0" placeholder="(same)" /></Field.Field>
				</div>
				<Button onclick={share} disabled={sharing || !auditor}>
					{#if sharing}<Spinner data-icon="inline-start" />{/if}
					Create disclosure
				</Button>
			</section>

			<section class="card pane">
				<div class="pane-head"><ScaleIcon class="size-4 text-primary" /><h2 class="pane-title">Package</h2></div>
				{#if pkg}
					<p class="hint">Hand this to the auditor out-of-band (they verify it in “Review”).</p>
					<Textarea value={pkg} readonly rows={12} class="font-mono text-xs" />
					<div class="flex justify-end"><CopyButton text={pkg} label="Copy package" /></div>
				{:else}
					<div class="empty">The disclosure package will appear here once created.</div>
				{/if}
			</section>
		</div>
	{:else}
		<div class="cols" in:fly={{ y: 12, duration: 280, easing: cubicOut }}>
			<section class="card pane">
				<div class="pane-head"><ScaleIcon class="size-4 text-primary" /><h2 class="pane-title">Verify a disclosure</h2></div>
				<p class="hint">Auditor side — needs no wallet. Paste the package the owner shared; each note's opening is re-checked against its on-chain commitment.</p>
				<Field.Field>
					<Field.Label>Disclosure package (JSON)</Field.Label>
					<Textarea bind:value={inputPkg} rows={10} placeholder={'{ … }'} class="font-mono text-xs" />
				</Field.Field>
				{#if pkgMeta.owner}
					<div class="kv"><span class="k">Owner</span><span class="v font-mono">{truncate(pkgMeta.owner, 6, 6)}</span></div>
				{/if}
				<Button onclick={verify} disabled={verifying || !inputPkg.trim()}>
					{#if verifying}<Spinner data-icon="inline-start" />{/if}
					Verify package
				</Button>
			</section>

			<section class="card pane">
				<div class="pane-head"><CheckCircle2Icon class="size-4 text-primary" /><h2 class="pane-title">Disclosed transactions</h2></div>
				{#if result}
					<div class="result-top">
						<div><span class="rt-n">{result.notes.length}</span><span class="rt-l">notes verified</span></div>
						<div><span class="rt-n">{fmtVal(result.total)}</span><span class="rt-l">total (base units)</span></div>
						<div><span class="rt-n">{result.fromEpoch === result.toEpoch ? result.fromEpoch : `${result.fromEpoch}–${result.toEpoch}`}</span><span class="rt-l">epoch{result.fromEpoch === result.toEpoch ? '' : 's'}</span></div>
					</div>
					<div class="notes">
						{#each result.notes as n (n.commitment)}
							<div class="note">
								<span class="n-epoch">epoch {n.epoch}</span>
								<span class="n-val font-mono">{fmtVal(n.value)}</span>
								<span class="n-cm font-mono">{truncate(n.commitment, 6, 4)}</span>
								<CheckCircle2Icon class="size-3.5 text-primary" />
							</div>
						{/each}
					</div>
				{:else}
					<div class="empty">Verify a package to list its disclosed, on-chain-checked notes here.</div>
				{/if}
			</section>
		</div>
	{/if}
</div>

<style>
	.hub {
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
	:global(.modes) {
		display: grid;
		grid-template-columns: 1fr 1fr;
		width: 200px;
	}
	.cols {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 18px;
		flex: 1;
		min-height: 0;
	}
	@media (max-width: 1000px) {
		.hub {
			overflow-y: auto;
		}
		.cols {
			grid-template-columns: 1fr;
			flex: none;
		}
	}
	.card {
		border: 1px solid var(--border);
		border-radius: var(--radius-3xl);
		background: var(--card);
		/* backdrop-filter: blur(16px);
		-webkit-backdrop-filter: blur(16px);
		box-shadow:
			0 1px 0 0 color-mix(in oklch, white 4%, transparent) inset,
			0 8px 24px -12px rgb(0 0 0 / 0.6); */
	}
	.pane {
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 18px 20px;
		min-height: 0;
		overflow-y: auto;
	}
	.pane-head {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.pane-title {
		font-family: var(--font-heading);
		font-size: 1rem;
		font-weight: 600;
	}
	.hint {
		font-size: 0.75rem;
		line-height: 1.45;
		color: var(--muted-foreground);
	}
	.opt {
		font-size: 0.6875rem;
		font-weight: 400;
		color: var(--muted-foreground);
		margin-left: 4px;
	}
	.two {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}
	.empty {
		display: grid;
		place-items: center;
		flex: 1;
		min-height: 120px;
		font-size: 0.8125rem;
		color: var(--muted-foreground);
		text-align: center;
		padding: 0 24px;
	}
	.kv {
		display: flex;
		align-items: center;
		justify-content: space-between;
		font-size: 0.8125rem;
	}
	.kv .k {
		color: var(--muted-foreground);
	}
	.result-top {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 10px;
		padding: 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: color-mix(in oklch, var(--primary) 6%, transparent);
	}
	.result-top > div {
		display: flex;
		flex-direction: column;
	}
	.rt-n {
		font-family: var(--font-heading);
		font-size: 1.25rem;
		font-weight: 600;
		font-variant-numeric: tabular-nums;
	}
	.rt-l {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
	}
	.notes {
		display: flex;
		flex-direction: column;
		gap: 6px;
		min-height: 0;
		overflow-y: auto;
	}
	.note {
		display: grid;
		grid-template-columns: auto 1fr auto auto;
		align-items: center;
		gap: 10px;
		padding: 8px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: color-mix(in oklch, var(--card) 50%, transparent);
		font-size: 0.8125rem;
	}
	.n-epoch {
		color: var(--muted-foreground);
		font-size: 0.75rem;
	}
	.n-val {
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	.n-cm {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
	}
</style>
