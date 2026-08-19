<script>
	// Jednotný štítek „tohle patří systému" — stejný vzhled napříč
	// sekcemi (Tasks, Programs, Files, Po spuštění).
	import { ShieldCheck, Wrench } from 'lucide-svelte';

	/// `compact` = jen ikonka s tooltipem (do řádků seznamů); jinak celý
	/// badge s textem (do detailů). `level`: 'mandatory' je výchozí a
	/// chová se přesně jako dřív, aby stávající volání nic nepocítila.
	/// `title` přebije výchozí popisek — Files tam posílá konkrétní
	/// důvod ze `systemPathInfo`.
	let { compact = false, level = 'mandatory', label = null, title = null } = $props();

	// Dvě úrovně = dvě ikony a dvě barvy. Víc odstínů by v seznamu
	// o desítkách řádků dělalo šum, ne informaci.
	const managed = $derived(level === 'managed');
	const Icon = $derived(managed ? Wrench : ShieldCheck);
	const text = $derived(label ?? (managed ? 'spravuje Windows' : 'součást Windows'));
	const tip = $derived(
		title ??
			(managed
				? 'Systémová data — uklidit je umí nástroj Windows, ne ruční smazání'
				: 'Povinná součást Windows / Microsoft — neodinstalovávat')
	);
</script>

<span class="sysb" class:compact class:full={!compact} class:managed title={tip}>
	<Icon size={compact ? 14 : 15} />
	{#if !compact}{text}{/if}
</span>

<style>
	.sysb {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--net-down);
		vertical-align: -2px;
		flex: none;
	}
	/* Úroveň „spravuje Windows" jantarově: v aplikaci je --warn barva
	   pro „pozor, ale řešitelné" — přesně tenhle případ. */
	.sysb.managed {
		color: var(--warn);
	}
	.sysb.full {
		font-size: var(--fs-xs);
		text-transform: uppercase;
		letter-spacing: 0.06em;
		border: 1px solid color-mix(in srgb, currentColor 40%, transparent);
		border-radius: 999px;
		padding: 2px 8px;
	}
</style>
