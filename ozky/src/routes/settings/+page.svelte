<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import AssetSelect from '$lib/components/shared/AssetSelect.svelte';
	import ProvingOverlay from '$lib/components/shared/ProvingOverlay.svelte';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import AddressField from '$lib/components/shared/AddressField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Spinner } from '$lib/components/ui/spinner';
	import { Badge } from '$lib/components/ui/badge';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Alert from '$lib/components/ui/alert';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import FileKeyIcon from '@lucide/svelte/icons/file-key';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import ScaleIcon from '@lucide/svelte/icons/scale';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import ShieldIcon from '@lucide/svelte/icons/shield';
	import LayersIcon from '@lucide/svelte/icons/layers';
	import CoinsIcon from '@lucide/svelte/icons/coins';
	import ServerIcon from '@lucide/svelte/icons/server';
	import LogOutIcon from '@lucide/svelte/icons/log-out';
	import { toast } from 'svelte-sonner';
	import { api, errMessage, type RecoveryExport } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';
	import { settings } from '$lib/settings.svelte';

	let funding = $state('');

	// Reveal spending key (active account).
	let keyOpen = $state(false);
	let spendingKey = $state('');
	let loadingKey = $state(false);

	// Export all recovery phrases (every account in the wallet).
	let recoveryOpen = $state(false);
	let phrases = $state<RecoveryExport[]>([]);
	let loadingPhrases = $state(false);

	// Account provisioning (funder + trustlines) — retry for accounts made while the funder was down.
	let provisioning = $state(false);

	// ASP enrollment + note consolidation.
	let enrolling = $state(false);
	let consolidateAsset = $state('USDC');
	let consolidating = $state(false);

	// Headless keeper endpoint (cloud push target; empty = local-task-only).
	let keeperUrl = $state('');
	let keeperToken = $state('');
	let savingKeeper = $state(false);
	let localKeeper = $state(false);
	let togglingLocal = $state(false);

	// Logout (wipe device) — typed confirmation guard.
	let logoutOpen = $state(false);
	let logoutConfirm = $state('');
	let loggingOut = $state(false);

	const allPhrasesText = $derived(
		phrases.map((p) => `${p.label}\n${p.mnemonic}`).join('\n\n')
	);

	onMount(async () => {
		try {
			funding = await api.fundingAddress();
		} catch (e) {
			toast.error('Could not load address', { description: errMessage(e) });
		}
		try {
			keeperUrl = await api.keeperEndpoint();
		} catch {
			keeperUrl = '';
		}
		try {
			localKeeper = await api.localKeeperStatus();
		} catch {
			localKeeper = false;
		}
	});

	async function revealKey() {
		keyOpen = true;
		if (spendingKey) return;
		loadingKey = true;
		try {
			spendingKey = await api.spendingKey();
		} catch (e) {
			toast.error('Could not load key', { description: errMessage(e) });
		} finally {
			loadingKey = false;
		}
	}

	async function exportPhrases() {
		recoveryOpen = true;
		if (phrases.length) return;
		loadingPhrases = true;
		try {
			phrases = await api.exportRecoveryPhrases();
		} catch (e) {
			toast.error('Could not export phrases', { description: errMessage(e) });
			recoveryOpen = false;
		} finally {
			loadingPhrases = false;
		}
	}

	async function provisionActive() {
		provisioning = true;
		const id = toast.loading('Funding account & setting up trustlines…');
		try {
			const r = await api.provisionAccount();
			await wallet.refreshPublicBalances();
			toast.success('Account set up', {
				id,
				description: r.account_created
					? `Funded with 10 XLM${r.added.length ? ` · trustlines ${r.added.join(', ')}` : ''}`
					: r.added.length
						? `Trustlines added: ${r.added.join(', ')}`
						: 'Already set up — nothing to do'
			});
		} catch (e) {
			toast.error('Could not set up account', { id, description: errMessage(e) });
		} finally {
			provisioning = false;
		}
	}

	async function enroll() {
		enrolling = true;
		const hash = await runAction('Enrolling into ASP', () => api.enroll(), {
			success: () => 'Enrolled',
			refresh: false
		});
		enrolling = false;
		if (hash) wallet.log({ kind: 'enroll', label: 'Enrolled into ASP', hash });
	}

	async function consolidate() {
		consolidating = true;
		const hash = await runAction('Consolidating notes', () => api.consolidate(consolidateAsset), {
			success: () => 'Notes consolidated'
		});
		consolidating = false;
		if (hash) wallet.log({ kind: 'send', label: `Consolidated ${consolidateAsset} notes`, hash });
	}

	async function toggleLocalKeeper() {
		togglingLocal = true;
		try {
			localKeeper = await api.setLocalKeeper(!localKeeper);
			toast.success(localKeeper ? 'Local keeper enabled' : 'Local keeper disabled', {
				description: localKeeper
					? 'A background task will submit armed payroll runs on schedule.'
					: undefined
			});
		} catch (e) {
			toast.error('Could not update local keeper', { description: errMessage(e) });
		} finally {
			togglingLocal = false;
		}
	}

	async function saveKeeper() {
		savingKeeper = true;
		try {
			await api.setKeeperEndpoint(keeperUrl.trim(), keeperToken.trim());
			keeperToken = '';
			toast.success(keeperUrl.trim() ? 'Keeper endpoint saved' : 'Keeper set to local-task-only');
		} catch (e) {
			toast.error('Could not save keeper endpoint', { description: errMessage(e) });
		} finally {
			savingKeeper = false;
		}
	}

	async function logout() {
		loggingOut = true;
		try {
			await wallet.logout();
			logoutOpen = false;
			await goto('/');
			toast.success('Logged out — wallet wiped from this device');
		} catch (e) {
			toast.error('Could not log out', { description: errMessage(e) });
		} finally {
			loggingOut = false;
		}
	}
</script>

<div class="hub">
	<div class="head">
		<div>
			<h1 class="title">Settings</h1>
			<p class="subtitle">Global preferences for this wallet — they apply to every account on this device.</p>
		</div>
	</div>

	<div class="grid">
		<!-- Recovery & keys ---------------------------------------------------->
		<section class="card pane span2">
			<div class="pane-head"><KeyRoundIcon class="size-4 text-primary" /><h2 class="pane-title">Recovery &amp; keys</h2></div>
			<p class="hint">
				Your 12-word recovery phrases are the <b>only</b> way to restore funds if this device is
				lost. Back up every account offline and never share a phrase.
			</p>
			<AddressField label="Active funding address" value={funding} loading={!funding} />
			<div class="row">
				<div class="row-text">
					<div class="row-title">Account setup</div>
					<p class="row-sub">Fund this account (10 XLM) and add USDC/EURC trustlines. Use if it wasn't set up at creation (e.g. the funder was offline).</p>
				</div>
				<Button variant="outline" onclick={provisionActive} disabled={provisioning}>
					{#if provisioning}<Spinner data-icon="inline-start" />{:else}<CoinsIcon data-icon="inline-start" />{/if}
					Set up
				</Button>
			</div>
			<div class="row">
				<div class="row-text">
					<div class="row-title">Recovery phrases</div>
					<p class="row-sub">Export all accounts' 12-word phrases for backup.</p>
				</div>
				<Button variant="outline" onclick={exportPhrases}>
					<FileKeyIcon data-icon="inline-start" />
					Export
				</Button>
			</div>
			<div class="row">
				<div class="row-text">
					<div class="row-title">Spending public key</div>
					<p class="row-sub">Shareable — give it to an ASP operator to enroll. Not a secret.</p>
				</div>
				<Button variant="outline" onclick={revealKey}>
					<KeyRoundIcon data-icon="inline-start" />
					Reveal
				</Button>
			</div>
		</section>

		<!-- Compliance --------------------------------------------------------->
		<section class="card pane">
			<div class="pane-head"><ScaleIcon class="size-4 text-primary" /><h2 class="pane-title">Compliance &amp; audit</h2></div>
			<p class="hint">
				Selective, read-only disclosure to an auditor, and joining the pool's approved set.
			</p>
			<div class="row">
				<div class="row-text">
					<div class="row-title">Disclosure &amp; audit</div>
					<p class="row-sub">Share scoped notes, or verify a package as an auditor.</p>
				</div>
				<Button variant="outline" onclick={() => goto('/auditor')}>
					<ShieldCheckIcon data-icon="inline-start" />
					Open
				</Button>
			</div>
			<div class="row">
				<div class="row-text">
					<div class="row-title">ASP enrollment</div>
					<p class="row-sub">Join the approved set (admin only).</p>
				</div>
				<Button variant="outline" onclick={enroll} disabled={enrolling}>
					{#if enrolling}<Spinner data-icon="inline-start" />{:else}<ShieldCheckIcon data-icon="inline-start" />{/if}
					Enroll
				</Button>
			</div>
		</section>

		<!-- Default privacy ---------------------------------------------------->
		<section class="card pane">
			<div class="pane-head"><ShieldIcon class="size-4 text-primary" /><h2 class="pane-title">Default privacy</h2></div>
			<p class="hint">
				The default mode for new payments — a timing strategy on this device only. Both modes
				look identical on-chain.
			</p>
			<div class="modes">
				<button type="button" class="mode" data-active={settings.privacyMode === 'instant'} onclick={() => (settings.privacyMode = 'instant')}>
					<ZapIcon class="size-4" />
					<span class="mode-text">
						<span class="mode-title">Instant</span>
						<span class="mode-sub">Submit right away</span>
					</span>
				</button>
				<button type="button" class="mode" data-active={settings.privacyMode === 'max'} onclick={() => (settings.privacyMode = 'max')}>
					<ShieldIcon class="size-4" />
					<span class="mode-text">
						<span class="mode-title">Maximum privacy</span>
						<span class="mode-sub">Delay before submit</span>
					</span>
				</button>
			</div>
		</section>

		<!-- Headless keeper ---------------------------------------------------->
		<section class="card pane span2">
			<div class="pane-head"><ServerIcon class="size-4 text-primary" /><h2 class="pane-title">Headless keeper</h2></div>
			<p class="hint">
				Where armed payroll runs are submitted from. Leave the endpoint blank for <b>local task
				only</b> (a background task on this machine). A cloud endpoint relays your pre-proved runs
				but never holds your spend key.
			</p>
			<div class="row">
				<div class="row-text">
					<div class="row-title">Run keeper locally</div>
					<p class="row-sub">A background task on this machine submits armed runs on schedule.</p>
				</div>
				<div class="flex items-center gap-2">
					{#if localKeeper}<Badge variant="secondary">On</Badge>{/if}
					<Button variant="outline" onclick={toggleLocalKeeper} disabled={togglingLocal}>
						{#if togglingLocal}<Spinner data-icon="inline-start" />{/if}
						{localKeeper ? 'Disable' : 'Enable'}
					</Button>
				</div>
			</div>
			<Input bind:value={keeperUrl} placeholder="https://keeper.example/submit (blank = local only)" />
			<Input bind:value={keeperToken} type="password" placeholder="Per-user token (write-only; leave blank to keep)" />
			<Button variant="outline" class="self-start" onclick={saveKeeper} disabled={savingKeeper}>
				{#if savingKeeper}<Spinner data-icon="inline-start" />{/if}
				Save cloud endpoint
			</Button>
		</section>

		<!-- Consolidate -------------------------------------------------------->
		<section class="card pane">
			<div class="pane-head"><LayersIcon class="size-4 text-primary" /><h2 class="pane-title">Consolidate notes</h2></div>
			<p class="hint">
				Merge a fragmented balance: collapse up to 4 of an asset's notes into one, so larger
				payments don't need to combine many notes. Stays fully shielded.
			</p>
			<div class="row">
				<div class="w-40"><AssetSelect bind:value={consolidateAsset} /></div>
				<Button variant="outline" onclick={consolidate} disabled={consolidating}>
					{#if consolidating}<Spinner data-icon="inline-start" />{:else}<LayersIcon data-icon="inline-start" />{/if}
					Consolidate
				</Button>
			</div>
		</section>

		<!-- Sign out ----------------------------------------------------------->
		<section class="card pane danger">
			<div class="pane-head"><LogOutIcon class="size-4 text-destructive" /><h2 class="pane-title">Sign out</h2></div>
			<p class="hint">
				Logging out <b>wipes this wallet from the device</b> — all accounts and local data. Unlike
				locking, it is irreversible without your recovery phrases. Export them first.
			</p>
			<Button variant="destructive" class="self-start" onclick={() => { logoutConfirm = ''; logoutOpen = true; }}>
				<LogOutIcon data-icon="inline-start" />
				Log out of this device
			</Button>
		</section>
	</div>
</div>

<!-- Reveal spending key ------------------------------------------------------>
<Dialog.Root bind:open={keyOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Spending public key</Dialog.Title>
			<Dialog.Description>Share this with an ASP operator to be enrolled. It is public — not a secret seed.</Dialog.Description>
		</Dialog.Header>
		<div class="flex flex-col gap-3">
			{#if loadingKey}
				<div class="grid place-items-center py-6"><Spinner /></div>
			{:else}
				<p class="rounded-lg border border-border bg-muted/50 p-3 font-mono text-xs break-all">{spendingKey}</p>
				<div class="flex justify-end"><CopyButton text={spendingKey} label="Copy key" /></div>
			{/if}
			<Alert.Root variant="destructive">
				<TriangleAlertIcon />
				<Alert.Description>Never share your 12-word recovery phrase. Only this public key is safe to share.</Alert.Description>
			</Alert.Root>
		</div>
	</Dialog.Content>
</Dialog.Root>

<!-- Export all recovery phrases ---------------------------------------------->
<Dialog.Root bind:open={recoveryOpen}>
	<Dialog.Content class="max-w-lg">
		<Dialog.Header>
			<Dialog.Title>Recovery phrases</Dialog.Title>
			<Dialog.Description>One 12-word phrase per account. Write them down and store them offline.</Dialog.Description>
		</Dialog.Header>
		<div class="flex flex-col gap-3">
			<Alert.Root variant="destructive">
				<TriangleAlertIcon />
				<Alert.Description>Anyone with a phrase controls that account's funds. Never paste it online or share it.</Alert.Description>
			</Alert.Root>
			{#if loadingPhrases}
				<div class="grid place-items-center py-6"><Spinner /></div>
			{:else}
				<div class="phrase-list">
					{#each phrases as p (p.index)}
						<div class="phrase">
							<div class="phrase-head">
								<span class="phrase-label">{p.label}</span>
								<CopyButton text={p.mnemonic} size="icon" />
							</div>
							<ol class="words">
								{#each p.mnemonic.split(/\s+/) as word, i (i)}
									<li><span class="word-n">{i + 1}</span>{word}</li>
								{/each}
							</ol>
						</div>
					{/each}
				</div>
				<div class="flex justify-end"><CopyButton text={allPhrasesText} label="Copy all phrases" /></div>
			{/if}
		</div>
	</Dialog.Content>
</Dialog.Root>

<!-- Logout confirmation ------------------------------------------------------>
<AlertDialog.Root bind:open={logoutOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Log out of this device?</AlertDialog.Title>
			<AlertDialog.Description>
				This permanently erases the wallet and all {wallet.accounts.length || ''} account{wallet.accounts.length === 1 ? '' : 's'}
				from this device. You can only restore from your recovery phrases. Type <b>LOG OUT</b> to confirm.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<Input bind:value={logoutConfirm} placeholder="LOG OUT" autocomplete="off" />
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={loggingOut}>Cancel</AlertDialog.Cancel>
			<Button variant="destructive" onclick={logout} disabled={loggingOut || logoutConfirm.trim().toUpperCase() !== 'LOG OUT'}>
				{#if loggingOut}<Spinner data-icon="inline-start" />{/if}
				Wipe &amp; log out
			</Button>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ProvingOverlay open={consolidating} title="Consolidating notes" />

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
		align-items: flex-start;
		justify-content: space-between;
		gap: 16px;
	}
	.title {
		font-family: var(--font-heading);
		font-size: 1.375rem;
		font-weight: 600;
		line-height: 1.1;
	}
	.subtitle {
		margin-top: 4px;
		font-size: 0.875rem;
		color: var(--muted-foreground);
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(2, 1fr);
		gap: 18px;
		align-content: start;
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding-bottom: 4px;
	}
	.span2 {
		grid-column: 1 / -1;
	}
	@media (max-width: 900px) {
		.grid {
			grid-template-columns: 1fr;
		}
		.span2 {
			grid-column: auto;
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
	}
	.pane.danger {
		border-color: color-mix(in oklch, var(--destructive) 40%, var(--border));
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
	.row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding: 10px 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: color-mix(in oklch, var(--card) 50%, transparent);
	}
	.row-text {
		min-width: 0;
	}
	.row-title {
		font-size: 0.8125rem;
		font-weight: 500;
	}
	.row-sub {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
	}
	.modes {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 8px;
	}
	.mode {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		text-align: left;
		transition: border-color 0.15s ease, background 0.15s ease;
	}
	.mode:hover {
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
	.mode[data-active='true'] {
		border-color: var(--primary);
		background: color-mix(in oklch, var(--primary) 8%, transparent);
		color: var(--primary);
	}
	.mode-text {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
	}
	.mode-title {
		font-size: 0.875rem;
		font-weight: 500;
	}
	.mode-sub {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
	}
	.phrase-list {
		display: flex;
		flex-direction: column;
		gap: 12px;
		max-height: 46vh;
		overflow-y: auto;
	}
	.phrase {
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 12px;
		background: color-mix(in oklch, var(--card) 50%, transparent);
	}
	.phrase-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 10px;
	}
	.phrase-label {
		font-size: 0.8125rem;
		font-weight: 600;
	}
	.words {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 6px;
		list-style: none;
		font-family: var(--font-mono, monospace);
		font-size: 0.75rem;
	}
	.words li {
		display: flex;
		align-items: baseline;
		gap: 6px;
		padding: 4px 8px;
		border-radius: var(--radius-sm);
		background: var(--muted);
	}
	.word-n {
		color: var(--muted-foreground);
		font-size: 0.625rem;
		min-width: 14px;
	}
</style>
