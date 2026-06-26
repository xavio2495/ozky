<script lang="ts">
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Tabs from '$lib/components/ui/tabs';
	import * as Alert from '$lib/components/ui/alert';
	import ScaleIcon from '@lucide/svelte/icons/scale';
	import { toast } from 'svelte-sonner';
	import { api, errMessage, type AuditResult } from '$lib/api';
	import { wallet, runAction } from '$lib/wallet.svelte';

	// Share
	let auditor = $state('');
	let fromEpoch = $state('');
	let toEpoch = $state('');
	let sharing = $state(false);
	let pkg = $state('');

	// Verify
	let inputPkg = $state('');
	let verifying = $state(false);
	let result = $state<AuditResult | null>(null);

	async function share() {
		if (!auditor.trim().startsWith('G')) {
			toast.error('Enter the auditor’s Stellar G… address');
			return;
		}
		const from = Number(fromEpoch) || 0;
		const to = Number(toEpoch) || from;
		if (to < from) {
			toast.error('“To epoch” must be ≥ “From epoch”');
			return;
		}
		sharing = true;
		const out = await runAction(
			'Creating disclosure',
			() => api.shareWithAuditor(auditor.trim(), from, to),
			{ success: () => 'Disclosure created', refresh: false }
		);
		sharing = false;
		if (out) {
			pkg = out;
			wallet.log({
				kind: 'disclose',
				label: `Shared with auditor`,
				detail: from === to ? `epoch ${from}` : `epochs ${from}–${to}`
			});
		}
	}

	async function verify() {
		verifying = true;
		try {
			result = await api.auditDisclosure(inputPkg.trim());
			toast.success('Disclosure verified');
		} catch (e) {
			result = null;
			toast.error('Could not verify package', { description: errMessage(e) });
		} finally {
			verifying = false;
		}
	}
</script>

<Workspace title="Auditor" subtitle="Selective, read-only disclosure — no spend authority">
	{#snippet main()}
		<Tabs.Root value="share" class="max-w-xl">
			<Tabs.List>
				<Tabs.Trigger value="share">Share</Tabs.Trigger>
				<Tabs.Trigger value="verify">Verify</Tabs.Trigger>
			</Tabs.List>

			<Tabs.Content value="share">
				<Card.Root>
					<Card.Header>
						<Card.Title>Disclose to an auditor</Card.Title>
						<Card.Description>
							Exports a scoped, read-only view and records an on-chain grant.
						</Card.Description>
					</Card.Header>
					<Card.Content>
						<Field.Group>
							<Field.Field>
								<Field.Label for="auditor">Auditor address</Field.Label>
								<Input id="auditor" bind:value={auditor} placeholder="G…" class="font-mono" />
							</Field.Field>
							<div class="grid grid-cols-2 gap-3">
								<Field.Field>
									<Field.Label for="fromEpoch">From epoch</Field.Label>
									<Input id="fromEpoch" bind:value={fromEpoch} type="number" min="0" placeholder="0" />
								</Field.Field>
								<Field.Field>
									<Field.Label for="toEpoch">To epoch</Field.Label>
									<Input id="toEpoch" bind:value={toEpoch} type="number" min="0" placeholder="(same)" />
								</Field.Field>
							</div>
							<Field.Description>
								Discloses only notes in this epoch range. Other epochs stay shielded — the auditor
								gets no key to them. Leave “to” blank to disclose a single epoch.
							</Field.Description>
							{#if pkg}
								<Field.Field>
									<Field.Label>Disclosure package</Field.Label>
									<Textarea value={pkg} readonly rows={4} class="font-mono text-xs" />
									<div class="flex justify-end">
										<CopyButton text={pkg} label="Copy package" />
									</div>
									<Field.Description>Hand this to the auditor out-of-band.</Field.Description>
								</Field.Field>
							{/if}
						</Field.Group>
					</Card.Content>
					<Card.Footer>
						<Button onclick={share} disabled={sharing || !auditor}>
							{#if sharing}<Spinner data-icon="inline-start" />{/if}
							Create disclosure
						</Button>
					</Card.Footer>
				</Card.Root>
			</Tabs.Content>

			<Tabs.Content value="verify">
				<Card.Root>
					<Card.Header>
						<Card.Title>Verify a disclosure</Card.Title>
						<Card.Description>Auditor side — needs no wallet. Paste a package to inspect it.</Card.Description>
					</Card.Header>
					<Card.Content>
						<Field.Group>
							<Field.Field>
								<Field.Label for="pkg">Disclosure package (JSON)</Field.Label>
								<Textarea id="pkg" bind:value={inputPkg} rows={5} placeholder="{'{'} … {'}'}" class="font-mono text-xs" />
							</Field.Field>
							{#if result}
								<Alert.Root>
									<ScaleIcon />
									<Alert.Title>Verified · {result.notes.length} note(s)</Alert.Title>
									<Alert.Description>
										Epochs {result.fromEpoch === result.toEpoch
											? result.fromEpoch
											: `${result.fromEpoch}–${result.toEpoch}`} · disclosed total:
										<b>{result.total}</b> base units.
									</Alert.Description>
								</Alert.Root>
							{/if}
						</Field.Group>
					</Card.Content>
					<Card.Footer>
						<Button onclick={verify} disabled={verifying || !inputPkg.trim()}>
							{#if verifying}<Spinner data-icon="inline-start" />{/if}
							Verify package
						</Button>
					</Card.Footer>
				</Card.Root>
			</Tabs.Content>
		</Tabs.Root>
	{/snippet}
</Workspace>
