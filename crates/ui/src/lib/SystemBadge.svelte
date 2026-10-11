<script>
	// Jednotný štítek „co to je a smí to pryč" — stejný vzhled napříč
	// sekcemi (Tasks, Programs, Files, Po spuštění, Vyhledávání).
	import { ShieldCheck, Wrench, Layers, Package, UserRound, RefreshCw } from 'lucide-svelte';
	import { STITKY, souhrnUrovne } from '$lib/mandatory.js';

	/// `compact` = jen ikonka s tooltipem (do řádků seznamů); jinak celý
	/// badge s textem (do detailů). `level`: 'mandatory' je výchozí a
	/// chová se přesně jako dřív, aby stávající volání nic nepocítila;
	/// neznámá úroveň se kreslí jako 'mandatory' (radši přísněji).
	/// `title` přebije výchozí popisek — Files tam posílá konkrétní
	/// důvod ze `systemPathInfo`. `certainty` má smysl jen u 'cache':
	/// mezipaměť, kterou výrobce nedoložil, je tlumenější.
	let {
		compact = false,
		level = 'mandatory',
		label = null,
		title = null,
		certainty = null
	} = $props();

	// Ikona říká, CO to je; barva, jestli to smí pryč:
	//   modrá    — Windows (nesahat),
	//   jantarová — pozor, ručně nemazat (správný nástroj, odinstalace,
	//              nemazat celé),
	//   šedá     — tvoje data (smazat jde, ale přijdeš o ně),
	//   zelená   — mezipaměť (smazat jde, vytvoří se znovu).
	// Víc odstínů by v seznamu o desítkách řádků dělalo šum, ne informaci.
	const IKONY = {
		mandatory: ShieldCheck,
		managed: Wrench,
		kontejner: Layers,
		aplikace: Package,
		data: UserRound,
		cache: RefreshCw
	};
	/// Původní tooltipy obou starých úrovní — beze změny, ať Tasks,
	/// Programs a Po spuštění vypadají jako dřív.
	const PUVODNI_TIP = {
		mandatory: 'Povinná součást Windows / Microsoft — neodinstalovávat',
		managed: 'Systémová data — uklidit je umí nástroj Windows, ne ruční smazání'
	};

	const lvl = $derived(Object.hasOwn(IKONY, level) ? level : 'mandatory');
	const Icon = $derived(IKONY[lvl]);
	const text = $derived(label ?? STITKY[lvl]);
	const tip = $derived(title ?? PUVODNI_TIP[lvl] ?? souhrnUrovne(lvl, certainty));
	const pozor = $derived(lvl === 'managed' || lvl === 'kontejner' || lvl === 'aplikace');
</script>

<span
	class="sysb"
	class:compact
	class:full={!compact}
	class:pozor
	class:data={lvl === 'data'}
	class:cache={lvl === 'cache'}
	class:nejiste={lvl === 'cache' && certainty !== 'high'}
	title={tip}
>
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
	   pro „pozor, ale řešitelné" — přesně tenhle případ. Kontejner
	   a věci programu taky: smazat je nejde ručně, ale jde to správnou
	   cestou (rozkliknout, odinstalovat). */
	.sysb.pozor {
		color: var(--warn);
	}
	/* Tvoje data: neutrálně. Není to varování ani pochvala — jen fakt,
	   že po smazání zmizí. */
	.sysb.data {
		color: var(--text-dim);
	}
	/* Mezipaměť klidně zeleně; bez dokladu výrobce tlumeněji, ať
	   „obvykle bezpečné" nevypadá stejně jisté jako doložené. */
	.sysb.cache {
		color: var(--ok);
	}
	.sysb.cache.nejiste {
		color: color-mix(in srgb, var(--ok) 55%, var(--text-dim));
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
