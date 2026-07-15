<script>
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';

	// Hlavní obrazovka je Tasks (DESIGN.md kap. 6).
	onMount(() => goto('/tasks', { replaceState: true }));
</script>