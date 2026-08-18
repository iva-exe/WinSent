<script>
	// Přepínač kategorií se scrollem — sdílený mezi Hardwarem a Ovladači.
	//
	// Existuje jako komponenta, ne jako zkopírovaný kus stránky: tohle
	// není jen pruh tlačítek, ale i plynulý scroll a dopočítávání toho,
	// která kategorie je zrovna vidět. Dvě kopie by se rozešly v chování,
	// a přesně to uživatel nahlásil — v Hardwaru to jelo plynule, v
	// Ovladačích to skákalo přes odkaz.
	//
	// `sections` jsou objekty { key, name, icon, items }.
	// `bodyEl` je scrollovaná oblast stránky.
	// `idPrefix` odlišuje kotvy, kdyby na stránce byly dvě navigace.
	let { sections = [], bodyEl = null, idPrefix = 'sect' } = $props();

	let activeCat = $state('');

	function anchorId(name) {
		return `${idPrefix}-${name}`;
	}

	// Pozice sekce uvnitř scrollované oblasti. Počítá se z rectů, ne
	// z offsetTop — ten je relativní k nejbližšímu pozicovanému rodiči,
	// což tady není `.body`, a scroll pak skákal úplně jinam.
	function offsetIn(el) {
		if (!el || !bodyEl) return 0;
		return el.getBoundingClientRect().top - bodyEl.getBoundingClientRect().top + bodyEl.scrollTop;
	}

	export function gotoSection(name) {
		const el = document.getElementById(anchorId(name));
		if (!el || !bodyEl) return;
		bodyEl.scrollTo({ top: Math.max(0, offsetIn(el) - 2), behavior: 'smooth' });
		activeCat = name;
	}

	// Aktivní kategorie podle pozice scrollu — přepínač se zvýrazňuje sám.
	let rafPending = false;
	export function onScroll() {
		if (rafPending || !bodyEl) return;
		rafPending = true;
		requestAnimationFrame(() => {
			rafPending = false;
			const y = bodyEl.scrollTop + 16;
			let current = sections[0]?.name ?? '';
			for (const s of sections) {
				if (offsetIn(document.getElementById(anchorId(s.name))) <= y) current = s.name;
			}
			activeCat = current;
		});
	}
</script>

<nav class="cats">
	{#each sections as s (s.key ?? s.name)}
		<button class="cat" class:on={activeCat === s.name} onclick={() => gotoSection(s.name)}>
			<s.icon size={16} />
			{s.name}
			<i>{s.items?.length ?? s.count ?? 0}</i>
		</button>
	{/each}
</nav>

<style>
	.cats {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		padding-bottom: 10px;
		border-bottom: 1px solid var(--border);
	}
	/* Stejný tvar jako segmentové přepínače v Programs a On start:
	   hranatý obdélník, aktivní = surface-hover + vnitřní rámeček,
	   počet mono ve faint barvě. Jedna aplikace, jeden jazyk. */
	.cat {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text-dim);
		font: inherit;
		font-size: 0.82rem;
		padding: 5px 11px;
		cursor: pointer;
		transition:
			color 0.12s ease,
			background 0.12s ease,
			box-shadow 0.12s ease;
	}
	.cat:hover {
		color: var(--text);
	}
	.cat.on {
		color: var(--text);
		background: var(--surface-hover);
		box-shadow: inset 0 0 0 1px var(--border-strong);
	}
	.cat i {
		font-style: normal;
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--text-faint);
		font-variant-numeric: tabular-nums;
	}
</style>
