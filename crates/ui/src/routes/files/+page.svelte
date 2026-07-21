<script>
	// Files (v4C, SPEC kap. 11.1–11.2): přehled svazků + SMART zdraví
	// a bleskové hledání přes NTFS MFT index. Jen čtení — mazání v8.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { HardDrive, Search, Folder, FileText, Thermometer, Activity } from 'lucide-svelte';

	let volumes = $state([]);
	let health = $state([]);
	let loadError = $state('');

	// MFT index + hledání.
	let indexLetter = $state(null); // písmeno s postaveným indexem
	let indexEntries = $state(0);
	let building = $state(null); // písmeno, které se právě staví
	let query = $state('');
	let hits = $state([]);
	let searching = $state(false);
	let debounce;

	const ATTR_DIR = 0x10;
	const ATTR_HIDDEN = 0x2;
	const ATTR_SYSTEM = 0x4;

	function fmtSize(b) {
		if (b == null) return '—';
		if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB';
		if (b >= 1e6) return (b / 1e6).toFixed(1) + ' MB';
		if (b >= 1e3) return (b / 1e3).toFixed(0) + ' kB';
		return b + ' B';
	}

	async function load() {
		try {
			const r = await invoke('query_volumes');
			volumes = r.volumes;
			health = r.health;
			loadError = '';
		} catch (e) {
			loadError = String(e);
		}
	}

	async function buildIndex(letter) {
		if (building) return;
		building = letter;
		try {
			indexEntries = await invoke('build_file_index', { letter });
			indexLetter = letter;
			hits = [];
			if (query.trim()) runSearch();
		} catch (e) {
			loadError = String(e);
		}
		building = null;
	}

	function onQueryInput() {
		clearTimeout(debounce);
		debounce = setTimeout(runSearch, 180);
	}

	async function runSearch() {
		if (!indexLetter || !query.trim()) {
			hits = [];
			return;
		}
		searching = true;
		try {
			hits = await invoke('search_files', {
				letter: indexLetter,
				query: query.trim(),
				limit: 200
			});
		} catch {
			hits = [];
		}
		searching = false;
	}

	async function openHit(h) {
		try {
			await invoke('open_path', { path: h.path });
		} catch {
			/* cesta mezitím zmizela */
		}
	}

	// Duplicity (SPEC 11.3): dvoufázová čtecí analýza on-demand.
	let dupRoot = $state('');
	let dups = $state(null);
	let dupsRunning = $state(false);

	async function runDups() {
		if (dupsRunning || !dupRoot.trim()) return;
		dupsRunning = true;
		dups = null;
		try {
			dups = await invoke('find_duplicates', {
				root: dupRoot.trim(),
				minSize: 1024 * 1024 // 1 MB — menší soubory nestojí za řeč
			});
		} catch {
			dups = [];
		}
		dupsRunning = false;
	}

	let dupWaste = $derived.by(() => {
		if (!dups) return 0;
		return dups.reduce((a, [size, paths]) => a + size * (paths.length - 1), 0);
	});

	function usedPct(v) {
		return v.total_bytes ? ((v.total_bytes - v.free_bytes) / v.total_bytes) * 100 : 0;
	}
	function barColor(pct) {
		if (pct >= 90) return 'var(--danger)';
		if (pct >= 75) return 'var(--warn)';
		return 'var(--ok)';
	}
	// Zdraví: NVMe used_pct → zbývající životnost.
	function healthColor(h) {
		if (h.critical) return 'var(--danger)';
		if ((h.used_pct ?? 0) >= 80) return 'var(--warn)';
		return 'var(--ok)';
	}

	onMount(() => {
		load();
		const t = setInterval(load, 30000);
		return () => clearInterval(t);
	});
</script>

<div class="page">
	<header class="head">
		<h1>Files</h1>
		<span class="sub">svazky · zdraví disků · bleskové hledání přes NTFS MFT</span>
	</header>

	{#if loadError}
		<p class="empty">{loadError}</p>
	{/if}

	<!-- ── Svazky ── -->
	<div class="vol-grid">
		{#each volumes as v (v.letter)}
			{@const pct = usedPct(v)}
			<div class="vol card">
				<div class="vol-head">
					<HardDrive size={16} />
					<span class="vol-letter">{v.letter}:</span>
					<span class="vol-label">{v.label || (v.fixed ? 'Místní disk' : 'Výměnný')}</span>
					<span class="vol-fs label-tech">{v.fs}</span>
				</div>
				<div class="bar">
					<div class="bar-fill" style:width="{pct}%" style:background={barColor(pct)}></div>
				</div>
				<div class="vol-nums mono">
					<span>{fmtSize(v.total_bytes - v.free_bytes)} / {fmtSize(v.total_bytes)}</span>
					<span class="dim">{fmtSize(v.free_bytes)} volných</span>
				</div>
				{#if v.fs === 'NTFS' && v.fixed}
					<button
						class="idx-btn"
						disabled={building != null}
						onclick={() => buildIndex(v.letter)}
					>
						{building === v.letter
							? 'stavím index…'
							: indexLetter === v.letter
								? `index: ${indexEntries.toLocaleString('cs-CZ')} záznamů`
								: 'Postavit index pro hledání'}
					</button>
				{/if}
			</div>
		{/each}
	</div>

	<!-- ── Zdraví fyzických disků (SMART/NVMe) ── -->
	<div class="health-row">
		{#each health as h (h.index)}
			<div class="hcard card">
				<span class="h-model">{h.model}</span>
				{#if h.temp_c != null}
					<span class="h-item" style:color={healthColor(h)}>
						<Activity size={13} />
						{100 - Math.min(h.used_pct ?? 0, 100)} % životnosti
					</span>
					<span class="h-item"><Thermometer size={13} /> {h.temp_c} °C</span>
					<span class="h-item dim">{h.power_on_hours} h provozu</span>
				{:else}
					<span class="h-item dim">SMART nedostupný (SATA — přijde později)</span>
				{/if}
			</div>
		{/each}
	</div>

	<!-- ── Hledání ── -->
	<section class="search card">
		<div class="s-head">
			<Search size={14} />
			<input
				placeholder={indexLetter
					? `hledat na ${indexLetter}: (instantně, ${indexEntries.toLocaleString('cs-CZ')} záznamů)`
					: 'nejdřív postav index svazku ↑'}
				bind:value={query}
				oninput={onQueryInput}
				disabled={!indexLetter}
			/>
			{#if searching}<span class="dim label-tech">hledám…</span>{/if}
		</div>
		{#if hits.length}
			<ul class="hits">
				{#each hits as h (h.path)}
					<li>
						<button
							class="hit"
							class:hidden-f={h.attrs & ATTR_HIDDEN}
							class:system-f={h.attrs & ATTR_SYSTEM}
							onclick={() => openHit(h)}
							title="Otevřít v Průzkumníku"
						>
							{#if h.attrs & ATTR_DIR}
								<Folder size={13} class="f-dir" />
							{:else}
								<FileText size={13} />
							{/if}
							<span class="hit-path mono">{h.path}</span>
							<span class="hit-size mono">{h.attrs & ATTR_DIR ? '' : fmtSize(h.size_bytes)}</span>
						</button>
					</li>
				{/each}
			</ul>
			<p class="legend">
				<span class="system-f">systémové</span> · <span class="hidden-f">skryté</span> ·
				max 200 nálezů · klik otevře v Průzkumníku · index se po 5 min nečinnosti uvolní
			</p>
		{:else if indexLetter && query.trim() && !searching}
			<p class="empty small">nic nenalezeno</p>
		{/if}
	</section>

	<!-- ── Duplicity (čtecí analýza; mazání až v8) ── -->
	<section class="card dups">
		<div class="s-head">
			<span class="label-tech">// duplicity (soubory ≥ 1 MB)</span>
			<input
				class="dup-input mono"
				placeholder="kořen, např. C:\Users\IVA\Downloads"
				bind:value={dupRoot}
			/>
			<button class="idx-btn dup-btn" disabled={dupsRunning} onclick={runDups}>
				{dupsRunning ? 'analyzuji…' : 'Najít duplicity'}
			</button>
		</div>
		{#if dups?.length}
			<p class="legend">
				{dups.length} skupin · zbytečně obsazeno {fmtSize(dupWaste)} — jen analýza,
				mazání přijde později (bezpečně, do koše)
			</p>
			<ul class="hits">
				{#each dups as [size, paths], gi (gi)}
					<li class="dup-group">
						<span class="dup-size mono">{fmtSize(size)} × {paths.length}</span>
						{#each paths as p (p)}
							<button class="hit" onclick={() => invoke('open_path', { path: p })}>
								<FileText size={12} />
								<span class="hit-path mono">{p}</span>
							</button>
						{/each}
					</li>
				{/each}
			</ul>
		{:else if dups && !dupsRunning}
			<p class="empty small">žádné duplicity nad 1 MB</p>
		{/if}
	</section>
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: 14px;
		height: 100%;
		min-height: 0;
	}
	.head {
		display: flex;
		align-items: baseline;
		gap: 12px;
	}
	.head h1 {
		font-size: 1.15rem;
		font-weight: 600;
	}
	.sub {
		color: var(--text-faint);
		font-size: 0.78rem;
	}
	.card {
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 12px 14px;
	}

	.vol-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
		gap: 10px;
	}
	.vol-head {
		display: flex;
		align-items: center;
		gap: 8px;
		color: var(--text);
		margin-bottom: 8px;
	}
	.vol-letter {
		font-family: var(--font-mono);
		font-weight: 500;
	}
	.vol-label {
		font-size: 0.82rem;
		color: var(--text-dim);
		flex: 1;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.vol-fs {
		font-size: 0.64rem;
	}
	.bar {
		height: 6px;
		border-radius: 3px;
		background: var(--surface-hover);
		overflow: hidden;
	}
	.bar-fill {
		height: 100%;
		border-radius: 3px;
		box-shadow: 0 0 6px color-mix(in srgb, currentColor 40%, transparent);
	}
	.vol-nums {
		display: flex;
		justify-content: space-between;
		font-size: 0.7rem;
		margin-top: 6px;
	}
	.idx-btn {
		margin-top: 8px;
		width: 100%;
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text-dim);
		font: inherit;
		font-size: 0.72rem;
		padding: 5px 8px;
		cursor: pointer;
	}
	.idx-btn:hover {
		color: var(--text);
		border-color: var(--border-strong);
	}
	.idx-btn:disabled {
		opacity: 0.6;
		cursor: wait;
	}

	.health-row {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
		gap: 10px;
	}
	.hcard {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.h-model {
		font-size: 0.82rem;
		font-weight: 500;
	}
	.h-item {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 0.76rem;
		font-family: var(--font-mono);
	}

	.search {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
	}
	.s-head {
		display: flex;
		align-items: center;
		gap: 8px;
		color: var(--text-dim);
	}
	.s-head input {
		flex: 1;
		background: none;
		border: none;
		outline: none;
		color: var(--text);
		font: inherit;
		font-size: 0.9rem;
		padding: 4px 0;
	}
	.hits {
		list-style: none;
		margin: 10px 0 0;
		padding: 0;
		overflow-y: auto;
		min-height: 0;
		flex: 1;
	}
	.hit {
		width: 100%;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 5px 8px;
		background: none;
		border: none;
		border-bottom: 1px dashed var(--border);
		color: var(--text);
		font: inherit;
		cursor: pointer;
		text-align: left;
	}
	.hit:hover {
		background: var(--surface-hover);
	}
	.hit-path {
		flex: 1;
		font-size: 0.78rem;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		direction: rtl;
		text-align: left;
	}
	.hit-size {
		font-size: 0.72rem;
		color: var(--text-dim);
	}
	.hit.system-f .hit-path,
	.legend .system-f {
		color: var(--warn);
	}
	.hit.hidden-f .hit-path,
	.legend .hidden-f {
		color: var(--text-faint);
	}
	.legend {
		font-size: 0.68rem;
		color: var(--text-faint);
		margin-top: 8px;
	}
	.dups {
		max-height: 40vh;
		overflow-y: auto;
	}
	.dup-input {
		flex: 1;
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text);
		font-size: 0.76rem;
		padding: 5px 8px;
		outline: none;
	}
	.dup-btn {
		width: auto;
		margin-top: 0;
		white-space: nowrap;
	}
	.dup-group {
		border-bottom: 1px dashed var(--border);
		padding: 6px 0;
	}
	.dup-size {
		font-size: 0.72rem;
		color: var(--warn);
		padding: 0 8px;
	}
	.mono {
		font-family: var(--font-mono);
	}
	.dim {
		color: var(--text-faint);
	}
	.empty {
		color: var(--text-faint);
		font-size: 0.85rem;
		padding: 12px;
	}
	.empty.small {
		padding: 8px 0;
	}
</style>
