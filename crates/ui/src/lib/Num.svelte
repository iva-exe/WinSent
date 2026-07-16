<script>
	// Animované číslo — hodnota plyne (tween) místo skoku. Používá se
	// napříč aplikací u všech proměnných KROMĚ řádků v listu procesů
	// (tam by stovky tweenů škodily výkonu).
	import { Tween } from 'svelte/motion';
	import { cubicOut } from 'svelte/easing';

	let { value = null, decimals = 1, suffix = '', format = null } = $props();

	const t = new Tween(0, { duration: 500, easing: cubicOut });

	$effect(() => {
		if (value != null) t.set(value);
	});

	const text = $derived(
		value == null ? '—' : format ? format(t.current) : `${t.current.toFixed(decimals)}${suffix}`
	);
</script>

{text}
