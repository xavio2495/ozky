<script lang="ts">
	import { slide } from 'svelte/transition';
	import { cubicInOut } from 'svelte/easing';
	import { nav } from '$lib/content/site';

	let { open = false, onClose }: { open?: boolean; onClose: () => void } = $props();

	// Which item is expanded to reveal its sub-pages (one at a time).
	let expanded = $state<string | null>(null);

	function close() {
		expanded = null;
		onClose();
	}

	function toggle(label: string) {
		expanded = expanded === label ? null : label;
	}
</script>

<!-- Backdrop -->
<div
	class="fixed inset-0 z-[60] bg-ink/40 transition-opacity duration-300"
	class:pointer-events-none={!open}
	class:opacity-0={!open}
	class:opacity-100={open}
	onclick={close}
	aria-hidden="true"
></div>

<!-- Floating popup anchored to the top-right -->
<aside
	class="fixed top-4 right-4 z-[70] flex min-h-[60dvh] w-[88dvw] min-w-0 flex-col rounded-[18px] bg-grey p-7 text-ink shadow-xl transition-all duration-300 ease-in-out sm:min-h-[40dvw] sm:w-[40dvw] sm:min-w-[340px]"
	style:transform-origin="top right"
	class:pointer-events-none={!open}
	class:scale-95={!open}
	class:opacity-0={!open}
	aria-hidden={!open}
>
	<div class="flex justify-end">
		<button onclick={close} class="mono flex items-center gap-2 text-[11px] text-ink">
			<span class="text-base leading-none">&times;</span> Close
		</button>
	</div>

	<!-- nav anchored to the bottom-left -->
	<nav class="mt-auto flex flex-col items-start">
		{#each nav as item (item.label)}
			{#if item.children}
				<button
					onclick={() => toggle(item.label)}
					class="font-display flex w-fit items-start text-[clamp(2rem,7vw,2.4rem)] leading-[1.15] font-semibold tracking-[-0.03em] text-ink lg:text-[clamp(1.4rem,2.6vw,2.2rem)]"
				>
					{item.label}<sup class="mt-5 ml-0.5 text-[0.5em] font-medium">{item.children.length}</sup>
				</button>
				{#if expanded === item.label}
					<ul
						transition:slide={{ duration: 320, easing: cubicInOut }}
						class="overflow-hidden pb-1 pl-1"
					>
						{#each item.children as c (c.label)}
							<li>
								<a
									href={c.href}
									onclick={close}
									class="font-display block py-0.5 text-[clamp(1.1rem,4vw,1.3rem)] font-medium text-ink lg:text-[clamp(0.95rem,1.4vw,1.2rem)]"
								>
									↳ {c.label}
								</a>
							</li>
						{/each}
					</ul>
				{/if}
			{:else}
				<a
					href={item.href}
					onclick={close}
					class="font-display w-fit text-[clamp(2rem,7vw,2.4rem)] leading-[1.15] font-semibold tracking-[-0.03em] text-ink lg:text-[clamp(1.4rem,2.6vw,2.2rem)]"
				>
					{item.label}
				</a>
			{/if}
		{/each}
	</nav>
</aside>
