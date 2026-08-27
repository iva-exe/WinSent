<script>
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { prvniViditelnaSekce } from '$lib/prefs.svelte.js';

	// Hlavní obrazovka je Tasks (DESIGN.md kap. 6) — pokud si ji
	// uživatel nevypnul v Nastavení; pak se jde na první zapnutou.
	onMount(() => goto(prvniViditelnaSekce('/tasks'), { replaceState: true }));
</script>