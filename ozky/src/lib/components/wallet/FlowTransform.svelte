<script lang="ts">
	import { onMount } from 'svelte';
	import { Handle, Position, useSvelteFlow, useUpdateNodeInternals, type NodeProps } from '@xyflow/svelte';
	import * as Select from '$lib/components/ui/select';
	import XIcon from '@lucide/svelte/icons/x';
	import { TOKENS, type TransformData } from './flow-nodes';

	let { id, data }: NodeProps = $props();
	const d = $derived(data as TransformData);
	const { updateNodeData, deleteElements } = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();
	onMount(() => requestAnimationFrame(() => updateNodeInternals(id)));
	const opLabel = { swap: 'Swap', shield: 'Shield', unshield: 'Unshield', consolidate: 'Consolidate' };
</script>

<div class="node">
	<Handle type="target" position={Position.Left} />
	<div class="node-head">
		<span>Transform</span>
		<button class="del nodrag" aria-label="Delete node" onclick={() => deleteElements({ nodes: [{ id }] })}>
			<XIcon class="size-3" />
		</button>
	</div>
	<div class="node-body">
		<Select.Root type="single" value={d.op} onValueChange={(v) => updateNodeData(id, { op: v })}>
			<Select.Trigger class="h-7 bg-popover text-xs">{opLabel[d.op]}</Select.Trigger>
			<Select.Content class="bg-popover">
				<Select.Item value="swap">Swap (AMM)</Select.Item>
				<Select.Item value="shield">Shield (deposit)</Select.Item>
				<Select.Item value="unshield">Unshield (withdraw)</Select.Item>
				<Select.Item value="consolidate">Consolidate notes</Select.Item>
			</Select.Content>
		</Select.Root>
		{#if d.op === 'swap'}
			<Select.Root type="single" value={d.toToken} onValueChange={(v) => updateNodeData(id, { toToken: v })}>
				<Select.Trigger class="h-7 bg-popover text-xs">→ {d.toToken}</Select.Trigger>
				<Select.Content class="bg-popover">
					{#each TOKENS as t (t)}<Select.Item value={t}>{t}</Select.Item>{/each}
				</Select.Content>
			</Select.Root>
		{/if}
	</div>
	<Handle type="source" position={Position.Right} />
</div>

<style>
	.node {
		position: relative;
		width: 150px;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: oklch(0.2 0 0);
		font-size: 0.75rem;
	}
	.node-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 5px 10px;
		font-weight: 600;
		background: color-mix(in oklch, var(--muted) 60%, transparent);
		border-radius: var(--radius-xl) var(--radius-xl) 0 0;
	}
	.del {
		display: grid;
		place-items: center;
		width: 18px;
		height: 18px;
		border-radius: 9999px;
		color: var(--muted-foreground);
	}
	.del:hover {
		color: var(--destructive);
		background: color-mix(in oklch, var(--destructive) 14%, transparent);
	}
	.node-body {
		display: flex;
		flex-direction: column;
		gap: 5px;
		padding: 8px;
	}
</style>
