<script lang="ts">
	import { fly } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import { toast } from 'svelte-sonner';
	import OnboardShell from './OnboardShell.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import { Input } from '$lib/components/ui/input';
	import * as Field from '$lib/components/ui/field';
	import * as InputOTP from '$lib/components/ui/input-otp';
	import LockOpenIcon from '@lucide/svelte/icons/lock-open';
	import { api, errMessage } from '$lib/api';
	import { wallet } from '$lib/wallet.svelte';

	let password = $state('');
	let otp = $state('');
	let busy = $state(false);

	const canSubmit = $derived(password.length > 0 && otp.length === 6);

	async function signIn() {
		if (!canSubmit) return;
		busy = true;
		try {
			await api.unlock(password, otp);
			password = '';
			otp = '';
			await wallet.refreshStatus();
			await wallet.loadSession();
			toast.success('Unlocked');
		} catch (e) {
			otp = '';
			toast.error('Could not unlock', { description: errMessage(e) });
		} finally {
			busy = false;
		}
	}
</script>

<div class="grid h-full w-full place-items-center p-6">
	<div in:fly={{ y: 14, duration: 300, easing: cubicOut }}>
		<OnboardShell>
			<Field.Group>
				<div class="flex flex-col gap-2">
					<h1 class="font-heading text-2xl font-semibold tracking-tight">Welcome back</h1>
					<p class="text-sm text-balance text-muted-foreground">
						Unlock your wallet with your password and authenticator code.
					</p>
				</div>
				<Field.Field>
					<Field.Label for="pw">Password</Field.Label>
					<Input
						id="pw"
						type="password"
						bind:value={password}
						placeholder="Your wallet password"
						onkeydown={(e) => e.key === 'Enter' && otp.length === 6 && signIn()}
					/>
				</Field.Field>
				<Field.Field>
					<Field.Label>Authenticator code</Field.Label>
					<InputOTP.Root maxlength={6} bind:value={otp} class="gap-3">
						{#snippet children({ cells })}
							<InputOTP.Group>
								{#each cells.slice(0, 3) as cell (cell)}
									<InputOTP.Slot {cell} />
								{/each}
							</InputOTP.Group>
							<InputOTP.Separator />
							<InputOTP.Group>
								{#each cells.slice(3, 6) as cell (cell)}
									<InputOTP.Slot {cell} />
								{/each}
							</InputOTP.Group>
						{/snippet}
					</InputOTP.Root>
				</Field.Field>
				<Field.Field>
					<Button onclick={signIn} disabled={busy || !canSubmit}>
						{#if busy}<Spinner data-icon="inline-start" />{:else}<LockOpenIcon
								data-icon="inline-start"
							/>{/if}
						Unlock
					</Button>
				</Field.Field>
				<Field.Description class="text-center">
					Lost access? Reinstall and restore from your 12-word recovery phrase.
				</Field.Description>
			</Field.Group>
		</OnboardShell>
	</div>
</div>
