<script lang="ts">
	import { onMount } from 'svelte';
	import Workspace from '$lib/components/layout/Workspace.svelte';
	import AddressField from '$lib/components/shared/AddressField.svelte';
	import * as Alert from '$lib/components/ui/alert';
	import InfoIcon from '@lucide/svelte/icons/info';
	import { api, errMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let receive = $state('');
	let funding = $state('');

	onMount(async () => {
		try {
			[receive, funding] = await Promise.all([api.receiveAddress(), api.fundingAddress()]);
		} catch (e) {
			toast.error('Could not load addresses', { description: errMessage(e) });
		}
	});
</script>

<Workspace title="Receive" subtitle="Share an address to get paid">
	{#snippet main()}
		<div class="grid max-w-2xl gap-4 sm:grid-cols-2">
			<AddressField
				label="Shielded code (private)"
				value={receive}
				loading={!receive}
				hint="Share with another ozky wallet to receive a fully private payment."
				qr
			/>
			<AddressField
				label="Funding address (public)"
				value={funding}
				loading={!funding}
				hint="Receive public funds from any wallet or exchange, then deposit to shield them."
				qr
			/>
		</div>
	{/snippet}

	{#snippet aside()}
		<Alert.Root>
			<InfoIcon />
			<Alert.Title>Which address?</Alert.Title>
			<Alert.Description>
				Use the <b>shielded code</b> for private ozky-to-ozky payments. Use the
				<b>funding address</b> to bring external funds in — they're public until you deposit them.
			</Alert.Description>
		</Alert.Root>
	{/snippet}
</Workspace>
