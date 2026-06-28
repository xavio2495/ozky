<script lang="ts">
	import { onMount } from 'svelte';
	import Logo from './Logo.svelte';

	let { onMenu }: { onMenu: () => void } = $props();

	let logoEl = $state<HTMLElement>();
	let mounted = $state(false); // drives the slide-in-from-edges entrance
	let logoTone = $state<'dark' | 'light'>('dark');
	let hidden = $state(false); // hide once the footer is ≥50% on screen

	// Read the tone of whatever section sits under the logo. Dark sections opt in
	// with data-nav="light"; everything else is dark-on-light. We use
	// elementsFromPoint and skip our own (pointer-events:none) header chrome so the
	// probe reads the section beneath, not the logo itself.
	function toneAt(el: HTMLElement | undefined): 'dark' | 'light' {
		if (!el) return 'dark';
		const r = el.getBoundingClientRect();
		const hits = document.elementsFromPoint(r.left + r.width / 2, r.top + r.height / 2);
		for (const h of hits) {
			if (h.closest('header')) continue;
			const nav = h.closest('[data-nav]');
			if (nav) return nav.getAttribute('data-nav') === 'light' ? 'light' : 'dark';
		}
		return 'dark';
	}

	function sync() {
		logoTone = toneAt(logoEl);
	}

	onMount(() => {
		requestAnimationFrame(() => (mounted = true));
		sync();
		window.addEventListener('scroll', sync, { passive: true });
		window.addEventListener('resize', sync);

		// Hide the navbar once the footer is at least half visible.
		const footer = document.querySelector('[data-footer]');
		const io = footer
			? new IntersectionObserver((entries) => (hidden = entries[0].isIntersecting), {
					threshold: 0.5
				})
			: null;
		if (footer && io) io.observe(footer);

		return () => {
			window.removeEventListener('scroll', sync);
			window.removeEventListener('resize', sync);
			io?.disconnect();
		};
	});
</script>

<!-- header is click-through so tone probing reads the section beneath it -->
<header
	class="pointer-events-none fixed inset-x-0 top-0 z-50 transition-all duration-500 ease-in-out"
	class:-translate-y-full={hidden}
	class:opacity-0={hidden}
>
	<div class="flex items-center justify-between px-8 py-6">
		<a
			bind:this={logoEl}
			href="/"
			class="pointer-events-auto transition-transform duration-700 ease-in-out hover:opacity-70"
			style:transform={mounted ? 'translateX(0)' : 'translateX(-340%)'}
			aria-label="ozky home"
		>
			<Logo size={30} tone={logoTone} />
		</a>

		<button
			onclick={onMenu}
			class="mono pointer-events-auto flex items-center gap-2 rounded-full bg-grey px-3 py-1.5 text-[8.5px] text-ink transition-transform duration-700 ease-in-out"
			style:transform={mounted ? 'translateX(0)' : 'translateX(340%)'}
			aria-label="Open menu"
		>
			<span class="flex w-4 flex-col gap-[3px]">
				<span class="h-[1.5px] w-full bg-ink"></span>
				<span class="h-[1.5px] w-2/3 bg-ink"></span>
			</span>
			<span class="hidden lg:inline">Menu</span>
		</button>
	</div>
</header>
