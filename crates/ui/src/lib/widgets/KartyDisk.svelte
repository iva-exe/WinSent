<script>
	// Dlaždice ze sekcí Files a Vyhledávání — místo na discích a index.
	//
	// Klik na řádek otevře cestu v Průzkumníku. Mazat se odsud nedá:
	// úklid je nevratná akce a ta patří do sekce, kde je vidět celý
	// seznam a plán, ne do dlaždice na přehledu.
	import { invoke } from '@tauri-apps/api/core';
	import { data } from './data.svelte.js';
	import { velikost, zaplneno, pred } from './pomoc.js';

	// Rozměry přicházejí v jednotkách mřížky: šířka ve sloupcích,
	// výška v řádcích (řádek je nízký, viz registr.js). Obsah se podle
	// nich rozhoduje, co se ještě vejde.
	let { typ, w = 1, h = 2 } = $props();

	let siroka = $derived(w >= 2);
	let velka = $derived(w >= 2 && h >= 4);

	let svazky = $derived((data.volumes?.volumes ?? []).filter((v) => v.fixed));
	let health = $derived(data.volumes?.health ?? []);
	let cleanup = $derived(data.cleanup);

	function otevri(path) {
		invoke('open_path', { path }).catch(() => {});
	}

	function barvaZaplneni(p) {
		if (p >= 90) return 'var(--danger)';
		if (p >= 75) return 'var(--warn)';
		return 'var(--ok)';
	}

	// ── SMART ────────────────────────────────────────────────────────
	// Disky bez jediné hodnoty se nevyhazují: „SMART nehlásí" je taky
	// odpověď a bez ní by seznam disků na přehledu neseděl s tím, co
	// je vidět v sekci.
	let smart = $derived(
		health.map((h) => ({
			...h,
			zna: h.used_pct != null || h.power_on_hours != null || h.temp_c != null
		}))
	);

	// ── co zabírá místo ──────────────────────────────────────────────
	let coZabira = $state('dirs');
	let disk = $state(null);
	let diskyVReportu = $derived.by(() => {
		const r = cleanup?.report;
		if (!r) return [];
		const s = new Set([...(r.big_dirs ?? []), ...(r.big_files ?? [])].map((x) => x[0]));
		return [...s].sort();
	});
	let velkeRadky = $derived.by(() => {
		const r = cleanup?.report;
		if (!r) return [];
		const zdroj = coZabira === 'dirs' ? (r.big_dirs ?? []) : (r.big_files ?? []);
		const d = disk ?? diskyVReportu[0];
		return zdroj.filter((x) => x[0] === d).slice(0, velka ? 8 : 4);
	});

	// ── duplicity ────────────────────────────────────────────────────
	// Report drží jen sto největších skupin, takže výsledek je spodní
	// odhad — a tak se to i musí napsat.
	let dupNavic = $derived.by(() => {
		const dups = cleanup?.report?.dups ?? [];
		return dups.reduce((s, [vel, cesty]) => s + vel * Math.max(0, cesty.length - 1), 0);
	});

	// ── indexy ───────────────────────────────────────────────────────
	// Svazek, který v seznamu indexování chybí, není „ještě se nestihl":
	// je to svazek, na kterém se hledat nedá, protože nemá NTFS.
	let indexy = $derived.by(() => {
		const idx = new Map((cleanup?.indexing ?? []).map((i) => [i[0], i]));
		return svazky.map((v) => {
			const i = idx.get(v.letter);
			if (!i) {
				return {
					letter: v.letter,
					stav: 'nelze',
					text: `${v.fs || 'jiný systém souborů'} — hledat se dá jen v NTFS`
				};
			}
			if (i[3]) return { letter: v.letter, stav: 'chyba', text: i[3] };
			if (i[2]) return { letter: v.letter, stav: 'hotovo', text: `${i[1].toLocaleString('cs-CZ')} položek` };
			return { letter: v.letter, stav: 'bezi', text: `indexuji… ${i[1].toLocaleString('cs-CZ')}` };
		});
	});
	let stavim = $state(null);
	async function postav(letter) {
		stavim = letter;
		try {
			await invoke('build_file_index', { letter });
		} catch {
			/* stav si při dalším tiku přečteme z query_cleanup */
		}
		stavim = null;
	}
</script>

{#if typ === 'svazky'}
	<ul class="w-list scroll">
		{#each svazky as v (v.letter)}
			{@const p = zaplneno(v)}
			<li>
				<button class="w-klik w-row" onclick={() => otevri(`${v.letter}:\\`)} title="Otevřít v Průzkumníku">
					<span class="pis w-mono">{v.letter}:</span>
					{#if siroka}<span class="w-name">{v.label || v.fs}</span>{/if}
					<span class="w-bar">
						<span class="w-fill" style:width="{p}%" style:background={barvaZaplneni(p)}></span>
					</span>
					<span class="w-mono w-dim">{velikost(v.free_bytes)} volných</span>
				</button>
			</li>
		{/each}
		{#if !svazky.length}<li class="w-empty">Svazky se ještě nenačetly.</li>{/if}
	</ul>
{:else if typ === 'smart'}
	<ul class="w-list scroll">
		{#each smart as h (h.index)}
			<li class="w-row">
				<span class="w-name">{h.model}</span>
				{#if h.zna}
					{#if h.used_pct != null}
						<span class="w-mono" style:color={h.used_pct >= 80 ? 'var(--warn)' : 'var(--ok)'}>
							{100 - Math.min(h.used_pct, 100)} % života
						</span>
					{/if}
					{#if siroka && h.power_on_hours != null}
						<span class="w-mono w-dim">{Math.round(h.power_on_hours / 24)} dní běhu</span>
					{/if}
				{:else}
					<span class="w-sub">SMART nehlásí</span>
				{/if}
			</li>
		{/each}
		{#if !smart.length}<li class="w-empty">Zdraví disků se ještě nenačetlo.</li>{/if}
	</ul>
{:else if typ === 'velke'}
	{#if cleanup?.report}
		<div class="w-segs">
			<button class="w-seg" class:on={coZabira === 'dirs'} onclick={() => (coZabira = 'dirs')}>Složky</button>
			<button class="w-seg" class:on={coZabira === 'files'} onclick={() => (coZabira = 'files')}>Soubory</button>
			{#each diskyVReportu as d (d)}
				<button
					class="w-seg"
					class:on={(disk ?? diskyVReportu[0]) === d}
					onclick={() => (disk = d)}>{d}:</button
				>
			{/each}
		</div>
		<ul class="w-list scroll">
			{#each velkeRadky as [, cesta, vel] (cesta)}
				<li>
					<button class="w-klik w-row" onclick={() => otevri(cesta)} title={cesta}>
						<span class="w-name">{cesta}</span>
						<span class="w-mono w-dim">{velikost(vel)}</span>
					</button>
				</li>
			{/each}
			{#if !velkeRadky.length}<li class="w-empty">Na tomhle svazku analýza nic velkého nenašla.</li>{/if}
		</ul>
	{:else if cleanup?.running}
		<span class="w-empty">Analyzuji obsah disků…</span>
	{:else}
		<span class="w-empty">Služba analýzu spustí sama krátce po startu.</span>
	{/if}
{:else if typ === 'duplicity'}
	{#if cleanup?.report}
		<span class="w-big">{velikost(dupNavic)}</span>
		<span class="w-sub">nejméně tolik drží kopie téhož</span>
		<span class="w-sub">
			{cleanup.report.dups.length} největších skupin · úklid je v sekci Files
		</span>
	{:else}
		<span class="w-empty">Analýza duplicit ještě neproběhla.</span>
	{/if}
{:else if typ === 'indexy'}
	<ul class="w-list scroll">
		{#each indexy as i (i.letter)}
			<li class="w-row">
				<span class="pis w-mono">{i.letter}:</span>
				<span class="w-name" class:w-dim={i.stav === 'nelze'}>{i.text}</span>
				{#if i.stav === 'hotovo'}
					<span class="tecka ok"></span>
				{:else if i.stav === 'bezi'}
					<span class="tecka bezi"></span>
				{:else if i.stav === 'chyba'}
					<button class="w-akce" disabled={stavim === i.letter} onclick={() => postav(i.letter)}>
						Zkusit znovu
					</button>
				{/if}
			</li>
		{/each}
		{#if !indexy.length}<li class="w-empty">Stav indexů se ještě nenačetl.</li>{/if}
	</ul>
	{#if cleanup?.report?.finished_ts}
		<span class="w-sub">poslední analýza {pred(cleanup.report.finished_ts)}</span>
	{/if}
{/if}

<style>
	.pis {
		flex: none;
		width: 20px;
		color: var(--text-dim);
	}
	.tecka {
		flex: none;
		width: 7px;
		height: 7px;
		border-radius: 50%;
	}
	.tecka.ok {
		background: var(--ok);
		box-shadow: var(--glow-ok);
	}
	.tecka.bezi {
		background: var(--warn);
		box-shadow: var(--glow-warn);
	}
</style>
