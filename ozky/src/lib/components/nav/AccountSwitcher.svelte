<script lang="ts">
	import * as Popover from '$lib/components/ui/popover';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import AccountAvatar from './AccountAvatar.svelte';
	import { wallet } from '$lib/wallet.svelte';
	import { api, MAX_ACCOUNTS, errMessage } from '$lib/api';
	import { truncate } from '$lib/format';
	import { toast } from 'svelte-sonner';
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import CheckIcon from '@lucide/svelte/icons/check';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';

	let open = $state(false);
	let busy = $state(false);

	// New-account phrase reveal dialog.
	let revealOpen = $state(false);
	let newPhrase = $state('');

	// Funder + trustline provisioning for the freshly-created account (shown in the reveal dialog).
	let provisioning = $state(false);
	let provisionDone = $state(false);
	let provisionError = $state('');
	let provisionResult = $state<{ created: boolean; added: string[] } | null>(null);

	// Import dialog.
	let importOpen = $state(false);
	let importText = $state('');
	let importLabel = $state('');

	// Rename dialog.
	let renameOpen = $state(false);
	let renameIndex = $state(0);
	let renameValue = $state('');

	// Remove-account confirmation.
	let removeOpen = $state(false);
	let removeIndex = $state(0);
	let removeLabel = $state('');

	const active = $derived(wallet.activeAccount);
	const atLimit = $derived(wallet.accounts.length >= MAX_ACCOUNTS);
	const importWordCount = $derived(importText.trim().split(/\s+/).filter(Boolean).length);
	const newWords = $derived(newPhrase ? newPhrase.trim().split(/\s+/) : []);

	async function select(index: number) {
		if (index === active?.index) {
			open = false;
			return;
		}
		busy = true;
		try {
			await wallet.switchAccount(index);
			toast.success('Switched account');
		} catch (e) {
			toast.error('Could not switch', { description: errMessage(e) });
		} finally {
			busy = false;
			open = false;
		}
	}

	async function create() {
		busy = true;
		try {
			const created = await wallet.createAccount();
			newPhrase = created.mnemonic;
			open = false;
			revealOpen = true;
			// The new account is its own fresh Stellar account — fund it (10 XLM via the funder)
			// and add its USDC/EURC trustlines, same as onboarding. Runs while the user backs up
			// the phrase; status shows in the reveal dialog + a toast on completion.
			void provisionNew();
		} catch (e) {
			toast.error('Could not create account', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}

	async function provisionNew() {
		provisioning = true;
		provisionDone = false;
		provisionError = '';
		provisionResult = null;
		const id = toast.loading('Funding new account & setting up trustlines…');
		try {
			const r = await api.provisionAccount();
			provisionResult = { created: r.account_created, added: r.added };
			await wallet.refreshPublicBalances();
			toast.success('New account ready', {
				id,
				description: r.account_created
					? `Funded with 10 XLM${r.added.length ? ` · trustlines ${r.added.join(', ')}` : ''}`
					: r.added.length
						? `Trustlines added: ${r.added.join(', ')}`
						: 'Account already set up'
			});
		} catch (e) {
			provisionError = errMessage(e);
			toast.error('Account funding failed', { id, description: errMessage(e) });
		} finally {
			provisioning = false;
			provisionDone = true;
		}
	}

	async function maybeProvisionImported() {
		// Only auto-provision a BLANK imported account — one with no Stellar account on-chain
		// yet (no XLM, no trustlines), e.g. a freshly generated seed that was never funded.
		// importAccount already refreshed publicBalances for the now-active imported account;
		// an empty list means Horizon has no account (404). An established wallet is untouched.
		if (wallet.publicBalances.length === 0) {
			await provisionNew();
		}
	}

	function openImport() {
		importText = '';
		importLabel = '';
		open = false;
		importOpen = true;
	}

	async function doImport() {
		if (importWordCount !== 12) {
			toast.error('Enter all 12 words');
			return;
		}
		busy = true;
		try {
			await wallet.importAccount(importText.trim(), importLabel.trim() || undefined);
			importOpen = false;
			toast.success('Wallet imported');
			void maybeProvisionImported();
		} catch (e) {
			toast.error('Could not import', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}

	function openRename(index: number, label: string) {
		renameIndex = index;
		renameValue = label;
		open = false;
		renameOpen = true;
	}

	async function doRename() {
		busy = true;
		try {
			await wallet.renameAccount(renameIndex, renameValue.trim());
			renameOpen = false;
			toast.success('Account renamed');
		} catch (e) {
			toast.error('Could not rename', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}

	function openRemove() {
		removeIndex = renameIndex;
		removeLabel = renameValue.trim() || `Account ${renameIndex + 1}`;
		renameOpen = false;
		removeOpen = true;
	}

	async function doRemove() {
		busy = true;
		try {
			await wallet.removeAccount(removeIndex);
			removeOpen = false;
			toast.success('Account removed');
		} catch (e) {
			toast.error('Could not remove account', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}
</script>

<Popover.Root bind:open>
	<Popover.Trigger>
		{#snippet child({ props })}
			<button {...props} class="trigger" disabled={busy}>
				<AccountAvatar seed={active?.address ?? ''} size={32} />
				<span class="min-w-0 flex-1 text-left">
					<span class="block truncate text-sm font-medium">{active?.label ?? 'Account'}</span>
					<span class="block truncate font-mono text-[11px] text-muted-foreground">
						{active ? truncate(active.address, 6, 6) : '…'}
					</span>
				</span>
				<ChevronsUpDownIcon class="size-4 text-muted-foreground" />
			</button>
		{/snippet}
	</Popover.Trigger>
	<Popover.Content class="w-[252px] p-1.5" align="start">
		<p class="px-2 py-1.5 text-xs font-medium text-muted-foreground">
			Accounts ({wallet.accounts.length}/{MAX_ACCOUNTS})
		</p>
		<div class="flex flex-col gap-0.5">
			{#each wallet.accounts as acct (acct.index)}
				<div class="row" class:active={acct.active}>
					<button class="row-main" onclick={() => select(acct.index)} disabled={busy}>
						<AccountAvatar seed={acct.address} size={28} />
						<span class="min-w-0 flex-1 text-left">
							<span class="block truncate text-sm">{acct.label}</span>
							<span class="block truncate font-mono text-[11px] text-muted-foreground">
								{truncate(acct.address, 6, 6)}
							</span>
						</span>
						{#if acct.active}<CheckIcon class="size-4 text-primary" />{/if}
					</button>
					<button
						class="rename-btn"
						title="Rename"
						aria-label="Rename {acct.label}"
						onclick={() => openRename(acct.index, acct.label)}
						disabled={busy}
					>
						<PencilIcon class="size-3.5" />
					</button>
				</div>
			{/each}
		</div>
		<div class="mt-1.5 flex flex-col gap-0.5 border-t border-border pt-1.5">
			<Button variant="ghost" size="sm" class="w-full justify-start gap-2" onclick={create} disabled={busy || atLimit}>
				<PlusIcon class="size-4" />
				Create account
			</Button>
			<Button variant="ghost" size="sm" class="w-full justify-start gap-2" onclick={openImport} disabled={busy || atLimit}>
				<DownloadIcon class="size-4" />
				Import wallet
			</Button>
			{#if atLimit}
				<p class="px-2 pt-1 text-[11px] text-muted-foreground">Limit reached ({MAX_ACCOUNTS}).</p>
			{/if}
		</div>
	</Popover.Content>
</Popover.Root>

<!-- New account: reveal its recovery phrase once -->
<Dialog.Root bind:open={revealOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Back up your new account</Dialog.Title>
			<Dialog.Description>
				This account has its own 12-word recovery phrase. Write it down — it's shown once.
			</Dialog.Description>
		</Dialog.Header>
		<ol class="phrase">
			{#each newWords as word, i}
				<li><span class="num">{i + 1}</span>{word}</li>
			{/each}
		</ol>
		<Alert.Root variant="destructive">
			<TriangleAlertIcon />
			<Alert.Description>Anyone with this phrase controls this account's funds.</Alert.Description>
		</Alert.Root>
		<div class="prov" class:ok={provisionDone && !provisionError} class:warn={!!provisionError}>
			{#if provisioning}
				<Spinner class="size-4" />
				<span>Funding account &amp; setting up trustlines…</span>
			{:else if provisionError}
				<TriangleAlertIcon class="size-4" />
				<span>Couldn't auto-fund this account — set it up later from Settings.</span>
			{:else if provisionDone}
				<CheckIcon class="size-4" />
				<span>
					{provisionResult?.created ? 'Funded with 10 XLM' : 'Account ready'}{provisionResult &&
					provisionResult.added.length
						? ` · trustlines ${provisionResult.added.join(', ')}`
						: ''}
				</span>
			{/if}
		</div>
		<Dialog.Footer>
			<CopyButton text={newWords.join(' ')} label="Copy phrase" />
			<Button onclick={() => (revealOpen = false)}>I've saved it</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Import an existing wallet by recovery phrase -->
<Dialog.Root bind:open={importOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Import a wallet</Dialog.Title>
			<Dialog.Description>Add an existing wallet by its 12-word recovery phrase.</Dialog.Description>
		</Dialog.Header>
		<Field.Group>
			<Field.Field>
				<Field.Label for="imp-label">Label (optional)</Field.Label>
				<Input id="imp-label" bind:value={importLabel} placeholder="e.g. Savings" />
			</Field.Field>
			<Field.Field>
				<Field.Label for="imp-phrase">Recovery phrase</Field.Label>
				<Textarea id="imp-phrase" bind:value={importText} rows={3} placeholder="word1 word2 word3 …" class="font-mono" />
				<Field.Description>{importWordCount}/12 words</Field.Description>
			</Field.Field>
		</Field.Group>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (importOpen = false)} disabled={busy}>Cancel</Button>
			<Button onclick={doImport} disabled={busy || importWordCount !== 12}>
				{#if busy}<Spinner data-icon="inline-start" />{/if}
				Import
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Rename an account -->
<Dialog.Root bind:open={renameOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Rename account</Dialog.Title>
			<Dialog.Description>Give this account a name you'll recognise.</Dialog.Description>
		</Dialog.Header>
		<Field.Group>
			<Field.Field>
				<Field.Label for="rename-label">Account name</Field.Label>
				<Input
					id="rename-label"
					bind:value={renameValue}
					placeholder="e.g. Savings"
					onkeydown={(e) => e.key === 'Enter' && doRename()}
				/>
			</Field.Field>
		</Field.Group>
		<Dialog.Footer>
			{#if wallet.accounts.length > 1}
				<Button
					variant="ghost"
					class="mr-auto gap-2 text-destructive hover:text-destructive"
					onclick={openRemove}
					disabled={busy}
				>
					<Trash2Icon class="size-4" />
					Remove account
				</Button>
			{/if}
			<Button variant="outline" onclick={() => (renameOpen = false)} disabled={busy}>Cancel</Button>
			<Button onclick={doRename} disabled={busy}>
				{#if busy}<Spinner data-icon="inline-start" />{/if}
				Save
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<!-- Confirm removing an account -->
<AlertDialog.Root bind:open={removeOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Remove {removeLabel}?</AlertDialog.Title>
			<AlertDialog.Description>
				This erases this account's seed and local data from this device. Its on-chain funds are
				<strong>not</strong> touched, but you can only restore access with its 12-word recovery
				phrase. Make sure you've backed it up.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={busy}>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action variant="destructive" onclick={doRemove} disabled={busy}>
				{#if busy}<Spinner data-icon="inline-start" />{/if}
				Remove account
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<style>
	.trigger {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		padding: 6px 12px 6px 6px;
		border: 1px solid var(--border);
		border-radius: 9999px;
		background: color-mix(in oklch, var(--card) 50%, transparent);
		transition: border-color 0.15s ease, background 0.15s ease;
	}
	.trigger:hover {
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
	.row {
		display: flex;
		align-items: center;
		gap: 2px;
		border-radius: var(--radius-sm);
		transition: background 0.12s ease;
	}
	.row:hover {
		background: var(--accent);
	}
	/* Recolour the account label, address, and check icon to read on the gold hover fill. */
	.row:hover .row-main,
	.row:hover .row-main span,
	.row:hover .row-main :global(svg) {
		color: var(--accent-foreground);
	}
	.row-main {
		display: flex;
		align-items: center;
		gap: 10px;
		flex: 1;
		min-width: 0;
		padding: 7px 8px;
	}
	.rename-btn {
		display: grid;
		place-items: center;
		width: 28px;
		height: 28px;
		flex-shrink: 0;
		margin-right: 6px;
		border-radius: var(--radius-sm);
		color: var(--muted-foreground);
		opacity: 0;
		transition: opacity 0.12s ease, color 0.12s ease, background 0.12s ease;
	}
	.row:hover .rename-btn {
		opacity: 1;
	}
	.rename-btn:hover {
		color: var(--primary);
		background: color-mix(in oklch, var(--primary) 12%, transparent);
	}
	.prov {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
	.prov.ok {
		color: var(--primary);
	}
	.prov.warn {
		color: var(--destructive);
	}
	.phrase {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 8px;
	}
	.phrase li {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		background: color-mix(in oklch, var(--muted) 50%, transparent);
		font-family: var(--font-mono, monospace);
		font-size: 0.8125rem;
	}
	.phrase .num {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
		min-width: 14px;
	}
</style>
