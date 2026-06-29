<script lang="ts">
	// On startup, discover the GCP backend services via the website /connect broker. If the
	// broker is unreachable or no backend service is up, show a dismissible popup telling the
	// user the service is unavailable and to contact the developer. (connect flow)
	import { onMount } from 'svelte';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import { api } from '$lib/api';

	let open = $state(false);
	let checking = $state(false);

	async function check() {
		checking = true;
		try {
			const d = await api.connectServices();
			open = !d.broker_reachable || !d.reachable;
		} catch {
			open = true;
		} finally {
			checking = false;
		}
	}

	onMount(check);
</script>

<AlertDialog.Root bind:open>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Service unavailable</AlertDialog.Title>
			<AlertDialog.Description>
				The ozky backend services are currently unreachable. Onboarding funding and cloud payroll
				may not work until they are back. Please try again shortly, or contact the developer if the
				problem persists.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<Button variant="outline" onclick={check} disabled={checking}>
				{#if checking}<Spinner class="size-4" />{/if}
				Retry
			</Button>
			<AlertDialog.Action>Dismiss</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
