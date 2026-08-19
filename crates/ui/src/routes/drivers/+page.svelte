<script>
	// Drivers (v10, SPEC kap. 6): co v počítači běží za ovladače, od koho
	// to je a jak je to staré.
	//
	// Sekce jen čte. Instalovat ovladače Winsent neumí a nebude — od toho
	// je Windows Update, který to má vyzkoušené a umí se vrátit zpátky.
	// Ukázat, který ovladač je z roku 2015 a od koho, je práce, kterou
	// Správce zařízení dělá mizerně a která nikoho nemůže rozbít.
	import { onMount, tick as nextTick } from 'svelte';
	import CategoryNav from '$lib/CategoryNav.svelte';
	import { mergeSame } from '$lib/mergesame.js';
	import { invoke } from '@tauri-apps/api/core';
	import { Search, Package, PackageCheck, TriangleAlert, ChevronRight } from 'lucide-svelte';
	import { byCategory } from '$lib/devcategory.js';

	let report = $state(null);
	let loadError = $state('');
	let filter = $state('');
	// vše | oem (doinstalované) | old (staré) | problem
	let segment = $state('all');

	async function load() {
		try {
			report = await invoke('query_drivers');
			loadError = '';
		} catch (e) {
			loadError = String(e);
		}
	}

	onMount(() => {
		load();
		// Ovladače se mění leda novým hardwarem; služba je navíc drží
		// pět minut v cache.
		const t = setInterval(load, 300000);
		return () => clearInterval(t);
	});

	// Rok z data ovladače. Systém ho hlásí v místním formátu, takže se
	// hledá ten díl, který na rok vypadá — nic jiného z data nepotřebujeme.
	function yearOf(d) {
		for (const n of (d.date ?? '').split(/\D+/)) {
			const v = Number(n);
			if (v > 1900 && v < 2200) return v;
		}
		return null;
	}

	const thisYear = new Date().getFullYear();
	// „Starý" má smysl jen u ovladačů od výrobců.
	//
	// Windows dávají VŠEM svým vestavěným ovladačům jedno a totéž datum
	// (21. 6. 2006) bez ohledu na to, kdy vznikly — naměřeno na tomhle
	// stroji u stovky z nich. Kdyby se počítaly, byl by seznam „starých"
	// zaplavený ovladači, se kterými není nic špatně, a ten jeden
	// skutečně zastaralý od výrobce by v tom zapadl.
	const OLD_YEARS = 5;
	function isOld(d) {
		if (!d.third_party) return false;
		const y = yearOf(d);
		return y != null && thisYear - y >= OLD_YEARS;
	}

	let shown = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		return (report?.drivers ?? []).filter((d) => {
			if (segment === 'oem' && !d.third_party) return false;
			if (segment === 'old' && !isOld(d)) return false;
			if (segment === 'problem' && !d.problem_code) return false;
			if (!q) return true;
			return `${d.device} ${d.provider} ${d.class_desc} ${d.inf} ${d.version}`
				.toLowerCase()
				.includes(q);
		});
	});

	// Rozdělení do kategorií — stejné jako v Hardwaru, ze sdíleného
	// modulu. Ovladač nese tytéž třídy jako zařízení, kterému patří,
	// takže „Periferie" znamená na obou obrazovkách totéž.
	// Signál „je to USB periferie" nesou ovladače v prefixu klíče
	// skupiny: prefix "dev:" vzniká v collector-hw právě z VID+PID,
	// ostatní prefixy jsou "chip:" a "id:".
	// Ovladače se slučují podle toho, co JE jeden ovladač: shodný název,
	// verze a poskytovatel. Tentýž ovladač obsluhuje klidně deset
	// zařízení a vypisovat ho desetkrát je jen šum — zajímavé je, že
	// existuje a jak je starý.
	let sections = $derived(
		byCategory(shown, (d) => (d.group_key ?? '').startsWith('dev:')).map((s) => ({
			...s,
			merged: mergeSame(s.items, (d) => `${d.device}|${d.version}|${d.provider}`)
		}))
	);

	let counts = $derived({
		all: report?.drivers?.length ?? 0,
		oem: report?.third_party ?? 0,
		old: (report?.drivers ?? []).filter(isOld).length,
		problem: report?.with_problem ?? 0
	});
	// Skok na problémový ovladač; opakovaný klik cykluje mezi nimi.
	// Stejné chování jako v Hardwaru — tam si na to uživatel zvykl.
	let bodyEl = $state(null);
	let catNav = $state(null);
	let flashId = $state('');
	let problemIdx = $state(-1);

	let problems = $derived.by(() => {
		const out = [];
		for (const s of sections) {
			s.merged.forEach((mg, i) => {
				if (mg.head.problem_code)
					out.push({ id: `drv-${s.name}-${i}`, label: mg.head.device });
			});
		}
		return out;
	});

	async function jumpToProblem() {
		if (!problems.length) return;
		problemIdx = (problemIdx + 1) % problems.length;
		const target = problems[problemIdx];
		await nextTick();
		const el = document.getElementById(target.id);
		if (!el) return;
		el.scrollIntoView({ behavior: 'smooth', block: 'center' });
		flashId = target.id;
		setTimeout(() => {
			if (flashId === target.id) flashId = '';
		}, 1600);
	}
</script>

<div class="page">
	<!-- Pevná hlavička: nescrolluje pryč. Stejná stavba jako Hardware —
	     nadpis, segmenty, filtr, a pod tím přepínač kategorií. -->
	<header class="head">
		<div class="head-top">
			<h1>Drivers</h1>
			<div class="seg">
				<button class:active={segment === 'all'} onclick={() => (segment = 'all')}>
					Vše <i>{counts.all}</i>
				</button>
				<button class:active={segment === 'oem'} onclick={() => (segment = 'oem')}>
					Od výrobců <i>{counts.oem}</i>
				</button>
				<button class:active={segment === 'old'} onclick={() => (segment = 'old')}>
					Zastaralé od výrobců <i>{counts.old}</i>
				</button>
				<button class:active={segment === 'problem'} onclick={() => (segment = 'problem')}>
					S problémem <i>{counts.problem}</i>
				</button>
			</div>
			{#if problems.length}
				<!-- Skok na problémový ovladač; opakovaný klik cykluje.
				     Stejné chování jako v Hardwaru. -->
				<button class="alarm" onclick={jumpToProblem}>
					<TriangleAlert size={16} />
					{problems.length}
					{problems.length === 1 ? "problém" : problems.length < 5 ? "problémy" : "problémů"}
					<span class="alarm-go">
						{problemIdx >= 0 ? problemIdx + 1 + "/" + problems.length : "ukázat"}
						<ChevronRight size={15} />
					</span>
				</button>
			{/if}
			<div class="filter">
				<Search size={14} />
				<input placeholder="hledat ovladač…" bind:value={filter} />
			</div>
		</div>
		{#if sections.length > 1}
			<CategoryNav bind:this={catNav} {sections} {bodyEl} idPrefix="sect" />
		{/if}
	</header>

	{#if loadError}
		<p class="empty">Nelze načíst ovladače: {loadError}</p>
	{:else if report}
		<div class="body" bind:this={bodyEl} onscroll={() => catNav?.onScroll()}>
			{#if !shown.length}
				<p class="empty">
					{filter.trim() ? 'Nic neodpovídá hledání.' : 'V tomhle zobrazení nic není.'}
				</p>
			{/if}
			{#each sections as s (s.key)}
				<section class="grp" id="sect-{s.name}">
				<h2 class="sect">
					<s.icon size={16} />
					{s.name}
					<span class="sect-n">{s.merged.length}</span>
				</h2>
				{#each s.merged as mg, i (mg.key)}
				{@const d = mg.head}
				{@const year = yearOf(d)}
				<article class="item" id="drv-{s.name}-{i}" class:flash={flashId === `drv-${s.name}-${i}`} class:bad={d.problem_code}>
					<div class="ico">
						{#if d.problem_code}
							<TriangleAlert size={19} />
						{:else if d.third_party}
							<Package size={19} />
						{:else}
							<PackageCheck size={19} />
						{/if}
					</div>
					<div class="info">
					<h3>
						{d.device}
						<!-- Tentýž ovladač obsluhuje klidně deset zařízení.
						     Vypisovat ho desetkrát je šum; kolika slouží,
						     je ale informace, která má cenu. -->
						{#if mg.count > 1}
							<span class="serves">slouží {mg.count} zařízením</span>
						{/if}
					</h3>
						<p class="vendor">{d.provider || '—'}</p>
						<!-- Mřížka popisek → hodnota. Dřív to byla řada štítků,
						     kde „verze 10.0.19041.1" a „Audio inputs and outputs"
						     vypadaly stejně a nešlo poznat, co je co. Pořadí
						     buněk je pevné, aby první sloupec napříč seznamem
						     držel vždycky tentýž druh údaje. -->
						<dl class="facts">
							{#if d.version}
								<div><dt>Verze</dt><dd>{d.version}</dd></div>
							{/if}
							{#if d.date}
								<div><dt>Datum</dt><dd>{d.date}</dd></div>
							{/if}
							{#if isOld(d) && year}
								<div><dt>Stáří</dt><dd class="warn">{thisYear - year} let</dd></div>
							{/if}
							{#if d.class_desc}
								<div><dt>Třída</dt><dd>{d.class_desc}</dd></div>
							{/if}
							{#if d.inf}
								<div><dt>INF</dt><dd class="mono">{d.inf}</dd></div>
							{/if}
						</dl>
					</div>
					<div class="side">
						{#if d.problem_code}
							<span class="pill bad"><TriangleAlert size={14} /> problém {d.problem_code}</span>
						{:else if d.third_party}
							<!-- Doinstalovaný ovladač není nic špatného — je to
							     obvykle ten správný od výrobce. Jen se hodí vědět,
							     že nepřišel s Windows. -->
							<span class="pill dim">od výrobce</span>
						{:else}
							<span class="pill quiet">z Windows</span>
						{/if}
					</div>
				</article>
				{/each}
				</section>
			{/each}

			<p class="note">
				Winsent ovladače jen čte. Instalovat ani vracet je neumí — od toho je Windows Update,
				který to má vyzkoušené na milionech strojů a v případě problému se umí vrátit zpátky.
				„Od výrobce" znamená, že ovladač doinstaloval někdo zvenčí (soubor <code>oem*.inf</code>);
				u grafiky, zvukovky nebo síťovky je to obvyklé a správné. Stáří je zvednutý prst,
				ne poplach: ovladač čipsetu z roku 2019 může být poslední, který kdy vyšel. Počítá se jen u ovladačů od výrobců — Windows dávají všem svým vestavěným jedno a totéž datum 21. 6. 2006 bez ohledu na to, kdy vznikly.
			</p>
		</div>
	{:else}
		<p class="empty">Načítám ovladače…</p>
	{/if}
</div>

<style>
	/* Mřížka popisek → hodnota.
	   Popisky jsou v jednom jazyce (mono verzálky) bez ohledu na to, že
	   hodnoty jsou různé typy — verze, datum, text. Sloupce se
	   přizpůsobí šířce, ale pořadí buněk je pevné, takže se dá očima
	   skákat po sloupci napříč celým seznamem. */
	.facts {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(150px, max-content));
		gap: 14px 34px;
		margin: 12px 0 0;
	}
	.facts dt {
		font-family: var(--font-mono);
		font-size: 0.6rem;
		letter-spacing: 0.05em;
		text-transform: uppercase;
		color: var(--text-faint);
	}
	.facts dd {
		margin: 2px 0 0;
		font-size: 0.82rem;
		color: var(--text);
		line-height: 1.35;
		word-break: break-word;
	}
	.facts dd.mono {
		font-family: var(--font-mono);
		font-size: 0.72rem;
		color: var(--text-dim);
	}
	/* Stáří je informace, ne chyba — jantarová, ne červená. */
	.facts dd.warn {
		color: var(--warn);
	}
	/* Pole pro hledání — bez tohohle mělo výchozí vzhled prohlížeče
	   a trčelo z jinak sladěné hlavičky. */
	.filter {
		display: flex;
		align-items: center;
		gap: 6px;
		margin-left: auto;
		padding: 5px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--surface);
		color: var(--text-dim);
	}
	.filter:focus-within {
		border-color: var(--border-strong);
		color: var(--text);
	}
	.filter input {
		background: none;
		border: none;
		outline: none;
		color: var(--text);
		font: inherit;
		font-size: 0.8rem;
		width: 180px;
	}
	.filter input::placeholder {
		color: var(--text-faint);
	}
	/* Segmentový přepínač — stejný jazyk jako Programs a On start.
	   Bez tohohle to byla holá tlačítka prohlížeče. */
	.seg {
		display: flex;
		gap: 2px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 2px;
		background: var(--surface);
	}
	.seg button {
		background: none;
		border: none;
		color: var(--text-dim);
		font: inherit;
		font-size: 0.76rem;
		padding: 4px 10px;
		border-radius: 3px;
		cursor: pointer;
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.seg button i {
		font-style: normal;
		font-family: var(--font-mono);
		font-size: 0.64rem;
		color: var(--text-faint);
	}
	.seg button.active {
		background: var(--surface-hover);
		color: var(--text);
		box-shadow: inset 0 0 0 1px var(--border-strong);
	}
	.seg button:hover:not(.active) {
		color: var(--text);
	}
	.page {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
		gap: 12px;
	}
	.head {
		display: flex;
		flex-direction: column;
		gap: 10px;
		flex: none;
	}
	.head-top {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding-right: 6px;
	}
	.grp {
		margin-top: 26px;
	}
	.grp:first-child {
		margin-top: 0;
	}
	.sect {
		position: sticky;
		top: 0;
		z-index: 1;
		display: flex;
		align-items: center;
		gap: 9px;
		margin: 0 0 11px;
		padding: 9px 2px 10px;
		font-family: var(--font-mono);
		font-size: 0.8rem;
		font-weight: 500;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--text-dim);
		background: linear-gradient(var(--bg) 80%, transparent);
	}
	.sect::after {
		content: '';
		flex: 1;
		height: 1px;
		background: var(--border);
	}
	.sect-n {
		font-weight: 400;
		font-size: 0.72rem;
		color: var(--text-faint);
		font-variant-numeric: tabular-nums;
	}
	.alarm {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		margin-left: auto;
		background: color-mix(in srgb, var(--danger) 12%, transparent);
		border: 1px solid color-mix(in srgb, var(--danger) 45%, transparent);
		border-radius: 999px;
		color: var(--danger);
		font: inherit;
		font-size: 0.84rem;
		padding: 8px 10px 8px 15px;
		cursor: pointer;
	}
	.alarm:hover {
		background: color-mix(in srgb, var(--danger) 20%, transparent);
	}
	.alarm-go {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		background: color-mix(in srgb, var(--danger) 22%, transparent);
		border-radius: 999px;
		padding: 2px 6px 2px 8px;
		font-variant-numeric: tabular-nums;
	}
	.item {
		display: grid;
		grid-template-columns: 40px minmax(0, 1fr) 180px;
		gap: 14px;
		align-items: start;
		padding: 14px 16px;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		margin-bottom: 8px;
		background: var(--surface);
		scroll-margin: 20px;
	}
	.item:hover {
		background: var(--surface-hover);
	}
	.item.bad {
		border-color: color-mix(in srgb, var(--danger) 45%, var(--border));
	}
	.item.flash {
		animation: flash 2.2s ease-out;
	}
	.item.bad .ico {
		color: var(--danger);
		background: color-mix(in srgb, var(--danger) 14%, transparent);
	}
	.ico {
		display: grid;
		place-items: center;
		width: 40px;
		height: 40px;
		border-radius: 11px;
		background: var(--surface-hover);
		color: var(--text-dim);
	}
	.info {
		min-width: 0;
	}
	/* Kolika zařízením ovladač slouží — drobné, ať nepřebije název. */
	.serves {
		font-family: var(--font-mono);
		font-size: 0.66rem;
		font-weight: 400;
		letter-spacing: 0.02em;
		color: var(--text-faint);
		padding: 1px 7px;
		border: 1px solid var(--border);
		border-radius: 999px;
		vertical-align: middle;
	}
	.vendor {
		margin: 3px 0 0;
		font-size: 0.82rem;
		color: var(--text-dim);
	}
	.side {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 7px;
		text-align: right;
	}
	.pill {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		font-size: 0.79rem;
		padding: 4px 11px;
		border-radius: 999px;
		border: 1px solid transparent;
		white-space: nowrap;
	}
	.pill.bad {
		color: var(--danger);
		background: color-mix(in srgb, var(--danger) 14%, transparent);
	}
	.pill.dim {
		color: var(--text-dim);
		background: var(--surface-hover);
	}
	.pill.quiet {
		color: var(--text-dim);
		background: var(--surface-hover);
		border-color: var(--border);
	}
	.pill.quiet::before {
		content: '';
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--ok);
	}
	.empty {
		color: var(--text-dim);
		font-size: 0.84rem;
		padding: 20px 0;
	}
	@keyframes flash {
		0%,
		55% {
			border-color: var(--danger);
			background: color-mix(in srgb, var(--danger) 14%, transparent);
		}
		100% {
			border-color: var(--border);
			background: var(--surface);
		}
	}
	/* Sekce kategorií — stejný jazyk jako Hardware a Security. */
	.head h1 {
		font-size: 1.2rem;
		font-weight: 600;
		margin: 0;
	}
	.item.bad .ico {
		color: var(--danger);
	}
	/* Stáří je informace, ne chyba — jantarová, ne červená. */
	.pill.bad {
		color: var(--danger);
		border-color: color-mix(in srgb, var(--danger) 55%, transparent);
	}
	.note {
		margin: 14px 0 0;
		font-size: 0.78rem;
		line-height: 1.55;
		color: var(--text-faint);
	}
	.note code {
		font-family: var(--font-mono);
		font-size: 0.72rem;
	}
</style>
