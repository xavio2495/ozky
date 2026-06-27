<script lang="ts">
	// Month grid with event chips (design-language §7 CalendarMonth). Out-of-month and
	// empty cells render with the hatch motif. Events are plotted on their day; tones map
	// to the gold ramp (+ destructive for past/expired).
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';

	export type CalEvent = {
		ts: number; // unix ms
		label: string;
		tone: 'gold' | 'bright' | 'due' | 'bad' | 'muted';
		selected?: boolean;
	};

	let {
		events = [],
		month = $bindable(new Date().getMonth()),
		year = $bindable(new Date().getFullYear())
	}: { events?: CalEvent[]; month?: number; year?: number } = $props();

	const monthName = $derived(
		new Date(year, month, 1).toLocaleDateString('en-US', { month: 'long', year: 'numeric' })
	);

	type Cell = { date: Date; inMonth: boolean; events: CalEvent[] };
	const cells = $derived.by<Cell[]>(() => {
		const first = new Date(year, month, 1);
		const start = new Date(first);
		start.setDate(first.getDate() - ((first.getDay() + 6) % 7)); // Monday-start
		const out: Cell[] = [];
		for (let i = 0; i < 42; i++) {
			const date = new Date(start);
			date.setDate(start.getDate() + i);
			const dayEvents = events.filter((e) => {
				const d = new Date(e.ts);
				return d.getFullYear() === date.getFullYear() && d.getMonth() === date.getMonth() && d.getDate() === date.getDate();
			});
			out.push({ date, inMonth: date.getMonth() === month, events: dayEvents });
		}
		return out;
	});

	const today = new Date();
	const isToday = (d: Date) =>
		d.getFullYear() === today.getFullYear() && d.getMonth() === today.getMonth() && d.getDate() === today.getDate();

	function step(dir: number) {
		let m = month + dir;
		let y = year;
		if (m < 0) {
			m = 11;
			y--;
		} else if (m > 11) {
			m = 0;
			y++;
		}
		month = m;
		year = y;
	}
	const dows = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
</script>

<div class="cal">
	<div class="cal-head">
		<button class="step" onclick={() => step(-1)} aria-label="Previous month"><ChevronLeftIcon class="size-4" /></button>
		<span class="month">{monthName}</span>
		<button class="step" onclick={() => step(1)} aria-label="Next month"><ChevronRightIcon class="size-4" /></button>
	</div>
	<div class="dow">
		{#each dows as d (d)}<span>{d}</span>{/each}
	</div>
	<div class="grid">
		{#each cells as c (c.date.getTime())}
			<div class="cell" class:out={!c.inMonth} class:today={isToday(c.date)}>
				<span class="day">{c.date.getDate()}</span>
				{#each c.events.slice(0, 2) as e (e.label + e.ts)}
					<span class="chip {e.tone}" class:sel={e.selected} title={e.label}>{e.label}</span>
				{/each}
				{#if c.events.length > 2}<span class="more">+{c.events.length - 2}</span>{/if}
			</div>
		{/each}
	</div>
</div>

<style>
	.cal {
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-height: 0;
		flex: 1;
	}
	.cal-head {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 14px;
	}
	.month {
		font-family: var(--font-heading);
		font-size: 0.9375rem;
		font-weight: 600;
		min-width: 150px;
		text-align: center;
	}
	.step {
		display: grid;
		place-items: center;
		width: 28px;
		height: 28px;
		border: 1px solid var(--border);
		border-radius: 9999px;
		color: var(--muted-foreground);
	}
	.step:hover {
		color: var(--primary);
		border-color: color-mix(in oklch, var(--primary) 35%, var(--border));
	}
	.dow {
		display: grid;
		grid-template-columns: repeat(7, 1fr);
		gap: 4px;
		font-size: 0.625rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--muted-foreground);
		text-align: center;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(7, 1fr);
		grid-auto-rows: minmax(0, 1fr);
		gap: 4px;
		flex: 1;
		min-height: 0;
	}
	.cell {
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding: 4px;
		border-radius: var(--radius-md);
		border: 1px solid transparent;
		background: color-mix(in oklch, var(--card) 50%, transparent);
		overflow: hidden;
	}
	.cell.out {
		background: repeating-linear-gradient(
			45deg,
			var(--muted),
			var(--muted) 5px,
			color-mix(in oklch, var(--primary) 7%, transparent) 5px,
			color-mix(in oklch, var(--primary) 7%, transparent) 10px
		);
	}
	.cell.today {
		border-color: color-mix(in oklch, var(--primary) 45%, var(--border));
	}
	.day {
		font-size: 0.6875rem;
		color: var(--muted-foreground);
		font-variant-numeric: tabular-nums;
	}
	.cell.out .day {
		opacity: 0.5;
	}
	.chip {
		font-size: 0.5625rem;
		line-height: 1.3;
		padding: 1px 4px;
		border-radius: 4px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.chip.gold {
		background: color-mix(in oklch, var(--chart-2) 28%, transparent);
		color: var(--foreground);
	}
	.chip.bright {
		background: color-mix(in oklch, var(--chart-1) 40%, transparent);
		color: var(--foreground);
	}
	.chip.due {
		background: var(--primary);
		color: var(--primary-foreground);
		font-weight: 600;
	}
	.chip.bad {
		background: color-mix(in oklch, var(--destructive) 30%, transparent);
		color: var(--foreground);
	}
	.chip.muted {
		background: var(--muted);
		color: var(--muted-foreground);
	}
	.chip.sel {
		outline: 1px solid var(--primary);
	}
	.more {
		font-size: 0.5625rem;
		color: var(--muted-foreground);
	}
</style>
