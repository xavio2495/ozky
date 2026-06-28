<script lang="ts">
	import Lander from '$lib/components/home/Lander.svelte';
	import Solutions from '$lib/components/home/Solutions.svelte';
	import Insights from '$lib/components/home/Insights.svelte';
	import Cta from '$lib/components/home/Cta.svelte';
	import { meta } from '$lib/content/site';
</script>

<svelte:head><title>{meta.title}</title></svelte:head>

<Lander />
<Solutions />
<Insights />
<Cta />
