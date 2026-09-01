<script>
	// Rám jedné dlaždice na Home.
	//
	// Widget uvnitř řeší jen svůj obsah; velikost, přetahování i ovládání
	// v režimu úprav jsou tady. Kdyby si to každý widget kreslil sám,
	// rozešly by se navzájem — a je jich přes čtyřicet.
	//
	// Přetahování NEjede přes HTML5 drag & drop. Ten ve WebView2 vyžaduje
	// draggable prvek, uvnitř kterého jsou tlačítka a vstupy — a ta si
	// stisk vezmou dřív, takže se dlaždice buď nechytila vůbec, nebo se
	// místo ní táhl text. Tohle je obyčejný pointer: stisk, práh čtyř
	// bodů (ať projde klik), a pak se pod kurzorem hledá cílová dlaždice.
	import { GripVertical, X, ArrowUpRight } from 'lucide-svelte';
	import {
		odeber,
		nastavSirku,
		nastavVysku,
		presunNa,
		ulozPoradi,
		minSirka,
		minVyska,
		mrizka,
		tah,
		RADEK,
		MEZERA,
		MAX_VYSKA
	} from './rozlozeni.svelte.js';

	let {
		/// Popis widgetu z registru.
		widget,
		/// Položka rozložení: { klic, id, w, h, text }.
		polozka,
		/// Je zapnutý režim úprav?
		edit = false,
		/// Obsah dlaždice.
		children
	} = $props();

	let el;

	/// Na úzkém okně se dlaždice nesmí roztáhnout přes víc sloupců,
	/// než kolik jich mřížka má — jinak by ji rozjela do šířky.
	let sirka = $derived(Math.min(polozka.w, mrizka.sloupcu));
	let tahne = $derived(tah.klic === polozka.klic);
	let meniVysku = $state(false);

	// ── přesouvání ───────────────────────────────────────────────────
	let zacatek = null;

	function stiskl(e) {
		if (!edit || e.button !== 0) return;
		// Ovládací prvky uvnitř dlaždice si stisk berou samy — jinak by
		// se přepínač metriky nedal kliknout, protože by pod prstem
		// odjela celá karta.
		if (e.target.closest('button, input, a, select, textarea, .bez-tahu')) return;
		zacatek = { x: e.clientX, y: e.clientY };
		window.addEventListener('pointermove', tahni);
		window.addEventListener('pointerup', pust, { once: true });
	}

	function tahni(e) {
		if (!zacatek) return;
		if (!tah.klic) {
			// Práh: pod čtyři body je to klik, ne tah.
			if (Math.hypot(e.clientX - zacatek.x, e.clientY - zacatek.y) < 4) return;
			tah.klic = polozka.klic;
			document.documentElement.classList.add('tahne-dlazdici');
		}
		posunPlochu(e);
		const pod = document.elementFromPoint(e.clientX, e.clientY)?.closest('[data-klic]');
		const cil = pod?.dataset.klic;
		if (cil) presunNa(polozka.klic, cil);
	}

	// Když se táhne k okraji, plocha se sama posune — jinak se dlaždice
	// nedá přesunout dál, než kam zrovna vidíš.
	function posunPlochu(e) {
		const plocha = el?.closest('.plocha');
		if (!plocha) return;
		const r = plocha.getBoundingClientRect();
		if (e.clientY < r.top + 48) plocha.scrollTop -= 12;
		else if (e.clientY > r.bottom - 48) plocha.scrollTop += 12;
	}

	function pust() {
		window.removeEventListener('pointermove', tahni);
		zacatek = null;
		if (tah.klic) {
			tah.klic = null;
			document.documentElement.classList.remove('tahne-dlazdici');
			ulozPoradi();
		}
	}

	// ── výška tažením za spodní hranu ────────────────────────────────
	let zacatekV = null;

	function chytHranu(e) {
		if (!edit || e.button !== 0) return;
		e.stopPropagation();
		zacatekV = { y: e.clientY, h: polozka.h };
		meniVysku = true;
		e.currentTarget.setPointerCapture?.(e.pointerId);
		window.addEventListener('pointermove', tahniHranu);
		window.addEventListener('pointerup', pustHranu, { once: true });
	}

	function tahniHranu(e) {
		if (!zacatekV) return;
		const kroky = Math.round((e.clientY - zacatekV.y) / (RADEK + MEZERA));
		nastavVysku(polozka.klic, zacatekV.h + kroky);
	}

	function pustHranu() {
		window.removeEventListener('pointermove', tahniHranu);
		zacatekV = null;
		meniVysku = false;
	}

	// ── šířka přepínačem ─────────────────────────────────────────────
	// Segmenty rovnou ukazují, jak široká dlaždice bude: první je úzký
	// proužek, poslední přes celou mřížku. Zvolený svítí, takže je
	// z přepínače vidět stav, ne jen „další velikost".
	let volbySirky = $derived(
		Array.from({ length: mrizka.sloupcu }, (_, i) => i + 1).filter(
			(n) => n >= minSirka(polozka.id)
		)
	);
</script>

<!-- V režimu úprav je celá dlaždice úchyt; mimo něj je to obyčejná
     karta a žádné posluchače nepotřebuje. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<article
	bind:this={el}
	data-klic={polozka.klic}
	class="dl"
	class:edit
	class:tahne
	class:holy={widget.holy}
	style:grid-column="span {sirka}"
	style:grid-row="span {polozka.h}"
	onpointerdown={stiskl}
>
	{#if edit || !widget.holy}
		<header class="hl">
			{#if edit}
				<span class="uchop" title="Přetažením přesuneš"><GripVertical size={14} /></span>
			{:else}
				<widget.ikona size={14} />
			{/if}
			<span class="nazev">{widget.nazev}</span>
			{#if edit}
				<span class="rozmer w-mono" class:vidno={meniVysku}>{polozka.w}×{polozka.h}</span>
				<span class="sirky bez-tahu" title="Šířka dlaždice">
					{#each volbySirky as n (n)}
						<button
							class="sirka"
							class:on={polozka.w === n}
							title="Šířka {n} {n === 1 ? 'sloupec' : n < 5 ? 'sloupce' : 'sloupců'}"
							onclick={() => nastavSirku(polozka.klic, n)}
						>
							<span class="pruh" style:width="{3 + n * 3}px"></span>
						</button>
					{/each}
				</span>
				<button class="ovl zrus" title="Odebrat z přehledu" onclick={() => odeber(polozka.klic)}>
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
	{/if}

	<div class="telo">
		{@render children()}
	</div>

	{#if edit}
		<!-- Spodní hrana = výška. Táhne se po řádcích mřížky, takže
		     dlaždice pořád sedí v mřížce a nikdy nezůstane „mezi". -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div
			class="hrana bez-tahu"
			class:aktivni={meniVysku}
			title="Tažením nastavíš výšku (max {MAX_VYSKA} řádků, min {minVyska(polozka.id)})"
			onpointerdown={chytHranu}
		>
			<span class="ryha"></span>
		</div>
	{/if}
</article>

<style>
	.dl {
		position: relative;
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
	/* Oddělovač není karta — je to čára s popiskem. Rám by z něj
	   udělal další dlaždici, a tím by přestal oddělovat. */
	.dl.holy {
		border-color: transparent;
		background: none;
		padding: 0 2px;
	}
	.dl.holy.edit {
		border-color: var(--border);
		border-style: dashed;
		padding: 6px 8px 8px;
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
		opacity: 0.55;
		border-color: var(--ok);
		box-shadow: inset 0 0 0 1px var(--ok);
		cursor: grabbing;
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
	.rozmer {
		flex: none;
		font-size: var(--fs-3xs);
		color: var(--text-faint);
		opacity: 0;
		transition: opacity 0.12s ease;
	}
	.rozmer.vidno,
	.dl:hover .rozmer {
		opacity: 1;
	}
	.sirky {
		display: flex;
		align-items: center;
		gap: 2px;
		flex: none;
	}
	.sirka {
		display: grid;
		place-items: center;
		width: 18px;
		height: 20px;
		padding: 0;
		border: 1px solid transparent;
		border-radius: var(--radius-sm);
		background: none;
		cursor: pointer;
	}
	.sirka .pruh {
		display: block;
		height: 8px;
		border-radius: 2px;
		background: var(--text-faint);
	}
	.sirka:hover {
		background: var(--surface-hover);
	}
	.sirka:hover .pruh {
		background: var(--text-dim);
	}
	.sirka.on {
		border-color: var(--border-strong);
		background: var(--surface-hover);
	}
	.sirka.on .pruh {
		background: var(--ok);
		box-shadow: var(--glow-ok);
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
	.hrana {
		position: absolute;
		left: 0;
		right: 0;
		bottom: 0;
		height: 10px;
		display: grid;
		place-items: center;
		cursor: ns-resize;
		touch-action: none;
	}
	.ryha {
		width: 26px;
		height: 3px;
		border-radius: 2px;
		background: var(--border-strong);
		transition: background 0.12s ease;
	}
	.hrana:hover .ryha,
	.hrana.aktivni .ryha {
		background: var(--ok);
		box-shadow: var(--glow-ok);
	}
</style>
