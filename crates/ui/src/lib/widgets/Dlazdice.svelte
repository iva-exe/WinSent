<script>
	// Rám jedné dlaždice na Home.
	//
	// Widget uvnitř řeší jen svůj obsah; hlavička, velikost, přetahování
	// i tlačítka v režimu úprav jsou tady. Kdyby si to každý widget
	// kreslil sám, rozešly by se navzájem — a je jich přes dvacet.
	import { GripVertical, X, Maximize2, ArrowUpRight } from 'lucide-svelte';
	import { VELIKOSTI } from './registr.js';
	import { odeber, dalsiVelikost, presun, mrizka } from './rozlozeni.svelte.js';

	let {
		/// Popis widgetu z registru.
		widget,
		/// Zvolená velikost („mala" … „siroka").
		velikost,
		/// Je zapnutý režim úprav?
		edit = false,
		/// Obsah dlaždice.
		children
	} = $props();

	let rozmer = $derived(VELIKOSTI[velikost] ?? VELIKOSTI.mala);
	/// Na úzkém okně se dlaždice nesmí roztáhnout přes víc sloupců,
	/// než kolik jich mřížka má — jinak by ji rozjela do šířky.
	let sirka = $derived(Math.min(rozmer.w, mrizka.sloupcu));
	let tahne = $state(false);
	let cil = $state(false);

	function zacniTah(e) {
		if (!edit) return;
		tahne = true;
		e.dataTransfer.effectAllowed = 'move';
		e.dataTransfer.setData('text/plain', widget.id);
	}

	function nad(e) {
		if (!edit) return;
		e.preventDefault();
		e.dataTransfer.dropEffect = 'move';
		cil = true;
	}

	function pust(e) {
		if (!edit) return;
		e.preventDefault();
		cil = false;
		const zdroj = e.dataTransfer.getData('text/plain');
		if (zdroj) presun(zdroj, widget.id);
	}
</script>

<!-- V režimu úprav je celá dlaždice cíl přetažení; mimo něj je to
     obyčejná karta a žádné posluchače nepotřebuje. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<article
	class="dl"
	class:edit
	class:tahne
	class:cil
	style:grid-column="span {sirka}"
	style:grid-row="span {rozmer.h}"
	draggable={edit}
	ondragstart={zacniTah}
	ondragend={() => (tahne = false)}
	ondragover={nad}
	ondragleave={() => (cil = false)}
	ondrop={pust}
>
	<header class="hl">
		{#if edit}
			<span class="uchop" title="Přetažením přesuneš"><GripVertical size={14} /></span>
		{:else}
			<widget.ikona size={14} />
		{/if}
		<span class="nazev">{widget.nazev}</span>
		{#if edit}
			<button
				class="ovl"
				title="Změnit velikost ({VELIKOSTI[velikost]?.popis ?? velikost})"
				onclick={() => dalsiVelikost(widget.id)}
			>
				<Maximize2 size={13} />
			</button>
			<button class="ovl zrus" title="Odebrat z přehledu" onclick={() => odeber(widget.id)}>
				<X size={14} />
			</button>
		{:else if widget.href}
			<!-- Odkaz do sekce, ze které dlaždice pochází. Šipka se
			     ukáže až při najetí, ať hlavička nešumí. -->
			<a class="skok" href={widget.href} title="Otevřít {widget.sekce}">
				<ArrowUpRight size={14} />
			</a>
		{/if}
	</header>
	<div class="telo">
		{@render children()}
	</div>
</article>

<style>
	.dl {
		display: flex;
		flex-direction: column;
		min-width: 0;
		min-height: 0;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 10px 12px 12px;
		overflow: hidden;
	}
	/* V režimu úprav dlaždice zjevně „drží v ruce": čárkovaný rám
	   a kurzor tahu. Žádné třesení — na přehledu o dvaceti kartách
	   je to spíš nevolnost než hravost. */
	.dl.edit {
		border-style: dashed;
		border-color: var(--border-strong);
		cursor: grab;
	}
	.dl.tahne {
		opacity: 0.4;
	}
	.dl.cil {
		border-color: var(--ok);
		box-shadow: inset 0 0 0 1px var(--ok);
	}
	.hl {
		display: flex;
		align-items: center;
		gap: 6px;
		flex: none;
		color: var(--text-dim);
		font-size: var(--fs-2xs);
		text-transform: uppercase;
		letter-spacing: 0.06em;
		margin-bottom: 8px;
	}
	.nazev {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.uchop {
		display: grid;
		place-items: center;
		color: var(--text-faint);
		cursor: grab;
	}
	.ovl {
		display: grid;
		place-items: center;
		flex: none;
		width: 20px;
		height: 20px;
		padding: 0;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: none;
		color: var(--text-dim);
		cursor: pointer;
	}
	.ovl:hover {
		background: var(--surface-hover);
		color: var(--text);
	}
	.ovl.zrus:hover {
		color: var(--danger);
		border-color: color-mix(in srgb, var(--danger) 50%, transparent);
	}
	.skok {
		display: grid;
		place-items: center;
		flex: none;
		color: var(--text-faint);
		opacity: 0;
		transition: opacity 0.12s ease;
	}
	.dl:hover .skok {
		opacity: 1;
	}
	.skok:hover {
		color: var(--text);
	}
	.telo {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		justify-content: center;
	}
</style>
