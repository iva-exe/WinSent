<script>
	// Vyhledávání souborů — jádro sekce Vyhledávání i spotlight lišty.
	//
	// Jedna komponenta pro obojí schválně: kdyby si lišta kreslila
	// vlastní seznam, rozešly by se v chování dvě věci, které jsou pro
	// uživatele táž funkce.
	//
	// Hledá se v MFT indexu, který drží služba (SPEC 11.2). Index čte
	// tabulku souborů NTFS napřímo, takže výsledky chodí v desítkách
	// milisekund i pro miliony souborů — to je to, co dělá „Everything"
	// rychlým a co běžné hledání ve Windows nedokáže.
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { Search, FolderOpen, File, Folder, Loader, CornerDownLeft } from 'lucide-svelte';
	import { openMenu, akceKopirovat, oddelovac } from '$lib/itemmenu.svelte.js';

	let {
		/// Kompaktní podoba pro spotlight lištu.
		compact = false,
		/// Zavolá se, když uživatel akci dokončil (lišta se pak schová).
		onhotovo = () => {}
	} = $props();

	let query = $state('');
	let hits = $state([]);
	let busy = $state(false);
	let chyba = $state('');
	let vybrany = $state(0);
	let vstup = $state(null);
	let svazky = $state([]);
	let indexStav = $state([]);

	/// FILE_ATTRIBUTE_DIRECTORY.
	const ATTR_DIR = 0x10;
	/// Kolik výsledků se tahá ze služby. Víc než se vejde na obrazovku
	/// nemá smysl — kdo hledá, upřesní dotaz, nescrolluje tisíce řádků.
	const LIMIT = 200;

	async function nacistSvazky() {
		try {
			const c = await invoke('query_cleanup');
			indexStav = c?.indexing ?? [];
			svazky = indexStav.filter(([, , hotovo]) => hotovo).map(([l]) => l);
		} catch {
			svazky = [];
		}
	}

	// Hledá se se zpožděním: psaní je rychlejší než dotaz přes pipe
	// a bez tlumení by se posílal dotaz na každé písmeno.
	let timer;
	function napsano() {
		clearTimeout(timer);
		vybrany = 0;
		const q = query.trim();
		if (q.length < 2) {
			hits = [];
			busy = false;
			return;
		}
		busy = true;
		timer = setTimeout(hledat, 90);
	}

	// Číslo běhu — odpověď staršího dotazu nesmí přepsat novější
	// výsledky. Uživatel píše rychle a odpovědi chodí na přeskáčku.
	let beh = 0;

	async function hledat() {
		const q = query.trim();
		const muj = ++beh;
		try {
			// Napříč všemi svazky naráz; služba je má v paměti zvlášť.
			const davky = await Promise.all(
				svazky.map((l) =>
					invoke('search_files', { letter: l, query: q, limit: LIMIT }).catch(() => [])
				)
			);
			if (muj !== beh) return;
			// Složky napřed, pak podle délky cesty: co je blíž kořeni,
			// bývá to hledané. Uvnitř abecedně, ať pořadí neposkakuje.
			hits = davky
				.flat()
				.sort((a, b) => {
					const da = (a.attrs & ATTR_DIR) !== 0;
					const db = (b.attrs & ATTR_DIR) !== 0;
					if (da !== db) return da ? -1 : 1;
					const la = a.path.split(/[\\/]/).length;
					const lb = b.path.split(/[\\/]/).length;
					if (la !== lb) return la - lb;
					return a.name.localeCompare(b.name, 'cs');
				})
				.slice(0, LIMIT);
			chyba = '';
		} catch (e) {
			if (muj !== beh) return;
			chyba = String(e);
			hits = [];
		}
		if (muj === beh) busy = false;
	}

	function fmtSize(b) {
		if (b == null) return '';
		if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB';
		if (b >= 1e6) return (b / 1e6).toFixed(1) + ' MB';
		if (b >= 1e3) return (b / 1e3).toFixed(0) + ' kB';
		return b + ' B';
	}

	/// Cesta bez posledního dílu — ten je už v názvu.
	function slozka(p) {
		const i = Math.max(p.lastIndexOf('\\'), p.lastIndexOf('/'));
		return i > 0 ? p.slice(0, i) : p;
	}

	/// Zvýrazní část názvu, která odpovídá dotazu. Bez toho není poznat,
	/// proč se řádek objevil.
	function casti(text) {
		const q = query.trim();
		if (!q) return [{ t: text, m: false }];
		const i = text.toLowerCase().indexOf(q.toLowerCase());
		if (i < 0) return [{ t: text, m: false }];
		return [
			{ t: text.slice(0, i), m: false },
			{ t: text.slice(i, i + q.length), m: true },
			{ t: text.slice(i + q.length), m: false }
		].filter((c) => c.t);
	}

	async function otevrit(h) {
		try {
			await invoke('open_path', { path: h.path });
		} catch (e) {
			chyba = String(e);
		}
		onhotovo();
	}

	function klavesa(e) {
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			vybrany = Math.min(vybrany + 1, hits.length - 1);
			doHledu();
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			vybrany = Math.max(vybrany - 1, 0);
			doHledu();
		} else if (e.key === 'Enter') {
			e.preventDefault();
			if (hits[vybrany]) otevrit(hits[vybrany]);
		}
	}

	// Vybraný řádek musí zůstat vidět při ovládání z klávesnice.
	function doHledu() {
		queueMicrotask(() => {
			document
				.querySelector('.fs-row.sel')
				?.scrollIntoView({ block: 'nearest' });
		});
	}

	function menuSoubor(e, h) {
		const jeSlozka = (h.attrs & ATTR_DIR) !== 0;
		const pripona = h.name.includes('.') ? h.name.split('.').pop().toLowerCase() : '';
		openMenu(e, {
			title: h.name,
			subtitle: slozka(h.path),
			// Celá cesta se do vyhledávače neposílá — je v ní jméno
			// uživatele a struktura jeho disku.
			hledat: [h.name],
			kontext: jeSlozka ? 'složka Windows' : 'soubor',
			items: [
				{ label: 'Otevřít v Průzkumníku', icon: 'folder', run: () => otevrit(h) },
				pripona
					? {
							label: `Co je přípona .${pripona}?`,
							icon: 'search',
							hint: `.${pripona}`,
							run: () => invoke('search_web', { query: `přípona souboru .${pripona}` })
						}
					: null,
				oddelovac,
				akceKopirovat(h.name, 'Kopírovat název'),
				akceKopirovat(h.path, 'Kopírovat celou cestu')
			]
		});
	}

	export function zaostri() {
		vstup?.focus();
		vstup?.select();
	}

	export function vycisti() {
		query = '';
		hits = [];
		vybrany = 0;
	}

	onMount(() => {
		nacistSvazky();
		zaostri();
		// Index se staví na pozadí; dokud není hotový, hlásí se to
		// a po dokončení se seznam svazků doplní sám.
		const t = setInterval(() => {
			if (svazky.length < indexStav.length || !indexStav.length) nacistSvazky();
		}, 2000);
		return () => {
			clearInterval(t);
			clearTimeout(timer);
		};
	});

	let stavIndexu = $derived.by(() => {
		if (!indexStav.length) return 'index se připravuje…';
		const chybi = indexStav.filter(([, , hotovo]) => !hotovo);
		if (!chybi.length) return '';
		return `indexuji ${chybi.map(([l]) => l + ':').join(' ')}`;
	});
</script>

<div class="fs" class:compact>
	<div class="fs-bar">
		<Search size={compact ? 20 : 17} />
		<input
			bind:this={vstup}
			bind:value={query}
			oninput={napsano}
			onkeydown={klavesa}
			placeholder={compact ? 'Hledat soubor nebo složku…' : 'hledat v souborech…'}
			spellcheck="false"
			autocomplete="off"
		/>
		{#if busy}
			<Loader size={16} class="fs-spin" />
		{:else if hits.length}
			<span class="fs-count label-tech">{hits.length}</span>
		{/if}
	</div>

	{#if stavIndexu}
		<p class="fs-note">{stavIndexu}</p>
	{/if}
	{#if chyba}
		<p class="fs-err">{chyba}</p>
	{/if}

	{#if hits.length}
		<ul class="fs-list">
			{#each hits as h, i (h.path)}
				{@const dir = (h.attrs & ATTR_DIR) !== 0}
				<li>
					<button
						class="fs-row"
						class:sel={i === vybrany}
						onclick={() => otevrit(h)}
						onmouseenter={() => (vybrany = i)}
						oncontextmenu={(e) => menuSoubor(e, h)}
					>
						<span class="fs-ico" class:dir>
							{#if dir}<Folder size={16} />{:else}<File size={16} />{/if}
						</span>
						<span class="fs-main">
							<span class="fs-name">
								{#each casti(h.name) as c}
									{#if c.m}<mark>{c.t}</mark>{:else}{c.t}{/if}
								{/each}
							</span>
							<span class="fs-path mono">{slozka(h.path)}</span>
						</span>
						{#if !dir && h.size_bytes != null}
							<span class="fs-size mono">{fmtSize(h.size_bytes)}</span>
						{/if}
						{#if i === vybrany}
							<span class="fs-enter"><CornerDownLeft size={14} /></span>
						{/if}
					</button>
				</li>
			{/each}
		</ul>
	{:else if query.trim().length >= 2 && !busy}
		<p class="fs-empty">Nic se nenašlo.</p>
	{:else if !compact}
		<p class="fs-empty">
			Napiš aspoň dva znaky. Hledá se v tabulce souborů NTFS, takže
			výsledky chodí okamžitě i na discích s miliony souborů.
		</p>
	{/if}
</div>

<style>
	.fs {
		display: flex;
		flex-direction: column;
		min-height: 0;
		height: 100%;
	}
	.fs-bar {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 9px 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		color: var(--text-dim);
		flex: none;
	}
	.compact .fs-bar {
		padding: 14px 16px;
		border: 0;
		border-bottom: 1px solid var(--border);
		border-radius: 0;
		background: none;
	}
	.fs-bar input {
		flex: 1;
		min-width: 0;
		border: 0;
		background: none;
		color: var(--text);
		font: inherit;
		font-size: var(--fs-lg);
		outline: none;
	}
	.compact .fs-bar input {
		font-size: 1.15rem;
	}
	.fs-count {
		flex: none;
		color: var(--text-faint);
	}
	.fs-note,
	.fs-err,
	.fs-empty {
		margin: 0.7rem 2px 0;
		font-size: var(--fs-sm);
		color: var(--text-dim);
	}
	.compact .fs-note,
	.compact .fs-err,
	.compact .fs-empty {
		margin: 0.8rem 18px;
	}
	.fs-err {
		color: var(--danger);
	}
	.fs-list {
		list-style: none;
		margin: 0.5rem 0 0;
		padding: 0;
		overflow-y: auto;
		min-height: 0;
	}
	.compact .fs-list {
		margin: 6px 0;
		padding: 0 6px;
	}
	.fs-row {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		padding: 7px 10px;
		border: 0;
		border-radius: var(--radius-sm);
		background: none;
		color: var(--text);
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.fs-row.sel {
		background: var(--surface-hover);
	}
	.fs-ico {
		flex: none;
		display: grid;
		place-items: center;
		color: var(--text-faint);
	}
	.fs-ico.dir {
		color: var(--warn);
	}
	.fs-main {
		display: flex;
		flex-direction: column;
		min-width: 0;
		flex: 1;
	}
	.fs-name {
		font-size: var(--fs-lg);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.fs-name mark {
		background: none;
		color: var(--ok);
		font-weight: 600;
	}
	.fs-path {
		font-size: var(--fs-xs);
		color: var(--text-faint);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		direction: rtl;
		text-align: left;
	}
	.fs-size {
		flex: none;
		font-size: var(--fs-xs);
		color: var(--text-dim);
	}
	.fs-enter {
		flex: none;
		color: var(--text-faint);
	}
	:global(.fs-spin) {
		animation: fs-spin 1.1s linear infinite;
	}
	@keyframes fs-spin {
		to {
			transform: rotate(360deg);
		}
	}
</style>
