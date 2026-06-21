<script lang="ts">
	import * as Popover from '$lib/components/ui/popover';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import { wallet } from '$lib/wallet.svelte';
	import { MAX_ACCOUNTS, errMessage } from '$lib/api';
	import { truncate } from '$lib/format';
	import { toast } from 'svelte-sonner';
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import CheckIcon from '@lucide/svelte/icons/check';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import WalletIcon from '@lucide/svelte/icons/wallet';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';

	let open = $state(false);
	let busy = $state(false);

	// New-account phrase reveal dialog.
	let revealOpen = $state(false);
	let newPhrase = $state('');

	// Import dialog.
	let importOpen = $state(false);
	let importText = $state('');
	let importLabel = $state('');

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
		} catch (e) {
			toast.error('Could not create account', { description: errMessage(e) });
		} finally {
			busy = false;
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
		} catch (e) {
			toast.error('Could not import', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}
</script>

<Popover.Root bind:open>
	<Popover.Trigger>
		{#snippet child({ props })}
			<button {...props} class="trigger" disabled={busy}>
				<span class="grid size-8 place-items-center rounded-md bg-primary/15 text-primary">
					<WalletIcon class="size-4" />
				</span>
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
				<button class="row" onclick={() => select(acct.index)} disabled={busy}>
					<span class="grid size-7 place-items-center rounded-md bg-muted text-xs font-semibold">
						{acct.index + 1}
					</span>
					<span class="min-w-0 flex-1 text-left">
						<span class="block truncate text-sm">{acct.label}</span>
						<span class="block truncate font-mono text-[11px] text-muted-foreground">
							{truncate(acct.address, 6, 6)}
						</span>
					</span>
					{#if acct.active}<CheckIcon class="size-4 text-primary" />{/if}
				</button>
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

<style>
	.trigger {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		padding: 8px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		background: color-mix(in oklch, var(--card) 50%, transparent);
		transition: border-color 0.15s ease, background 0.15s ease;
	}
	.trigger:hover {
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
	.row {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 8px;
		border-radius: var(--radius-sm);
		transition: background 0.12s ease;
	}
	.row:hover {
		background: var(--accent);
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
