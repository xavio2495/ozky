<script lang="ts">
	// Non-dismissable overlay shown while an action proves + submits. The commands are
	// synchronous with no progress events yet, so stages advance on a timed estimate; the
	// final stage holds until the call resolves and `open` flips false.
	import { Progress } from '$lib/components/ui/progress';
	import { fade, scale } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import CheckIcon from '@lucide/svelte/icons/check';
	import Loader2Icon from '@lucide/svelte/icons/loader-2';

	let { open = false, title = 'Working' }: { open?: boolean; title?: string } = $props();

	const stages = ['Building witness', 'Generating proof', 'Submitting transaction', 'Confirming on-chain'];
	const pct = [12, 55, 84, 95];
	let stage = $state(0);

	$effect(() => {
		if (!open) {
			stage = 0;
			return;
		}
		stage = 0;
		const timers = [
			setTimeout(() => (stage = 1), 1100),
			setTimeout(() => (stage = 2), 6500),
			setTimeout(() => (stage = 3), 9500)
		];
		return () => timers.forEach(clearTimeout);
	});
</script>

{#if open}
	<div class="overlay" transition:fade={{ duration: 160 }}>
		<div class="panel" transition:scale={{ start: 0.96, duration: 240, easing: cubicOut }}>
			<div class="glow"></div>
			<img src="/brand/icon.svg" alt="" class="mark" />
			<h2 class="font-heading text-lg font-semibold">{title}</h2>
			<p class="mt-1 text-sm text-muted-foreground">{stages[stage]}…</p>

			<div class="mt-5 w-full">
				<Progress value={pct[stage]} class="h-1.5" />
			</div>

			<ol class="steps">
				{#each stages as s, i}
					<li class="step" data-state={i < stage ? 'done' : i === stage ? 'active' : 'idle'}>
						<span class="dot">
							{#if i < stage}
								<CheckIcon class="size-3" />
							{:else if i === stage}
								<Loader2Icon class="size-3 animate-spin" />
							{/if}
						</span>
						{s}
					</li>
				{/each}
			</ol>

			<p class="mt-5 text-center text-xs text-muted-foreground">
				Proving runs locally — your inputs never leave this device.
			</p>
		</div>
	</div>
{/if}

<style>
	.overlay {
		position: fixed;
		inset: 0;
		z-index: 50;
		display: grid;
		place-items: center;
		background: color-mix(in oklch, var(--background) 70%, transparent);
		backdrop-filter: blur(8px);
	}
	.panel {
		position: relative;
		display: flex;
		flex-direction: column;
		align-items: center;
		width: 340px;
		padding: 30px 28px 24px;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: var(--card);
		box-shadow: 0 30px 80px -20px rgb(0 0 0 / 0.6);
		overflow: hidden;
	}
	.glow {
		position: absolute;
		top: -60px;
		left: 50%;
		width: 200px;
		height: 200px;
		transform: translateX(-50%);
		background: radial-gradient(circle, color-mix(in oklch, var(--primary) 30%, transparent), transparent 70%);
		pointer-events: none;
	}
	.mark {
		width: 44px;
		height: 44px;
		border-radius: 12px;
		margin-bottom: 14px;
		animation: pulse 1.8s ease-in-out infinite;
	}
	.steps {
		width: 100%;
		margin-top: 20px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.step {
		display: flex;
		align-items: center;
		gap: 10px;
		font-size: 0.8125rem;
		color: var(--muted-foreground);
		transition: color 0.2s ease;
	}
	.step[data-state='active'] {
		color: var(--foreground);
		font-weight: 500;
	}
	.step[data-state='done'] {
		color: var(--primary);
	}
	.dot {
		display: grid;
		place-items: center;
		width: 18px;
		height: 18px;
		border-radius: 999px;
		border: 1px solid var(--border);
		flex-shrink: 0;
	}
	.step[data-state='done'] .dot {
		background: color-mix(in oklch, var(--primary) 18%, transparent);
		border-color: var(--primary);
		color: var(--primary);
	}
	.step[data-state='active'] .dot {
		border-color: var(--primary);
		color: var(--primary);
	}
	@keyframes pulse {
		0%, 100% { transform: scale(1); opacity: 1; }
		50% { transform: scale(1.08); opacity: 0.85; }
	}
	@media (prefers-reduced-motion: reduce) {
		.mark { animation: none; }
	}
</style>
