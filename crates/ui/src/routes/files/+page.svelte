<script>
	// Files (v4E): úklid a zdraví disků. Služba po startu sama zindexuje
	// všechny NTFS svazky (progres níže) a najde duplicity, 0bajtové
	// soubory a temp junk. Rychlé hledání je bonus dole. Jen čtení —
	// mazání přijde v v8 (bezpečně, do koše).
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import {
		HardDrive,
		Thermometer,
		Activity,
		Copy,
		FileX,
		Loader,
		Trash2,
		TriangleAlert,
		FileType2,
		FolderTree
	} from 'lucide-svelte';
	import SystemBadge from '$lib/SystemBadge.svelte';
	import { isSystemPath } from '$lib/mandatory.js';

	let volumes = $state([]);
	let health = $state([]);
	let cleanup = $state(null); // { indexing, running, report }
	let loadError = $state('');

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
	function usedPct(v) {
		return v.total_bytes ? ((v.total_bytes - v.free_bytes) / v.total_bytes) * 100 : 0;
	}
	function barColor(pct) {
		if (pct >= 90) return 'var(--danger)';
		if (pct >= 75) return 'var(--warn)';
		return 'var(--ok)';
	}
	function healthColor(h) {
		if (h?.critical) return 'var(--danger)';
		if ((h?.used_pct ?? 0) >= 80) return 'var(--warn)';
		return 'var(--ok)';
	}

	// Karty: fyzický disk + jeho svazky (spojené přes disk_index).
	let diskCards = $derived.by(() => {
		const cards = health.map((h) => ({
			key: 'd' + h.index,
			model: h.model,
			health: h.temp_c != null ? h : null,
			vols: volumes.filter((v) => v.disk_index === h.index)
		}));
		const orphan = volumes.filter((v) => v.disk_index == null);
		if (orphan.length) {
			cards.push({ key: 'orphan', model: 'Ostatní svazky', health: null, vols: orphan });
		}
		return cards.filter((c) => c.vols.length || c.health);
	});

	let dupWaste = $derived.by(() => {
		const d = cleanup?.report?.dups ?? [];
		return d.reduce((a, [size, paths]) => a + size * (paths.length - 1), 0);
	});
	// Největší soubory/složky — přepínač svazku podle dat v reportu.
	let bigVolume = $state(null);
	let bigVolumes = $derived.by(() => {
		const r = cleanup?.report;
		if (!r) return [];
		return [...new Set([...r.big_dirs, ...r.big_files].map((x) => x[0]))].sort();
	});
	let bigShown = $derived(bigVolume ?? bigVolumes[0] ?? null);
	let bigDirs = $derived.by(() =>
		(cleanup?.report?.big_dirs ?? []).filter((x) => x[0] === bigShown)
	);
	let bigFiles = $derived.by(() =>
		(cleanup?.report?.big_files ?? []).filter((x) => x[0] === bigShown)
	);

	let indexingDone = $derived.by(() => (cleanup?.indexing ?? []).every((i) => i[2]));

	// ── Mazání do koše (v8) — nabídka, ne výchozí cesta. Plán ukáže
	// i to, kdo soubor drží; provedení jde přes validační vrstvu.
	let delPlan = $state(null); // { plan | deny, paths }
	let delBusy = $state(false);
	let delToast = $state(null);

	async function askDelete(paths, ev) {
		ev?.stopPropagation();
		delBusy = true;
		try {
			const r = await invoke('plan_delete', { paths });
			delPlan = r.plan_id != null ? { plan: r, paths } : { deny: r, paths };
		} catch (e) {
			delToast = { kind: 'deny', text: String(e) };
		}
		delBusy = false;
	}

	async function confirmDelete() {
		if (!delPlan?.plan || delBusy) return;
		delBusy = true;
		try {
			const r = await invoke('execute_plan', { planId: delPlan.plan.plan_id });
			delToast =
				r.verdict === 'allow' && r.outcome === 'ok'
					? { kind: 'ok', text: `přesunuto do koše (${delPlan.paths.length})` }
					: { kind: 'deny', text: r.deny_reason ?? `nepodařilo se (${r.outcome})` };
			load();
		} catch (e) {
			delToast = { kind: 'deny', text: String(e) };
		}
		delBusy = false;
		delPlan = null;
		setTimeout(() => (delToast = null), 4000);
	}

	async function openPath(path) {
		try {
			await invoke('open_path', { path });
		} catch {
			/* cesta zmizela */
		}
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
		try {
			cleanup = await invoke('query_cleanup');
		} catch {
			cleanup = null;
		}
	}

	onMount(() => {
		load();
		const t = setInterval(load, 4000);
		return () => clearInterval(t);
	});
</script>

<div class="page">
	<header class="head">
		<h1>Files</h1>
		<span class="sub">zdraví disků · úklid místa · duplicity</span>
	</header>

	{#if loadError}
		<p class="empty">{loadError}</p>
	{/if}

	<!-- ── Karty disků: kapacity + zdraví v jednom ── -->
	<div class="disk-grid">
		{#each diskCards as d (d.key)}
			<div class="disk card">
				<div class="d-head">
					<HardDrive size={18} />
					<span class="d-model">{d.model}</span>
					{#if d.health}
						<span class="d-health" style:color={healthColor(d.health)}>
							<Activity size={15} />
							{100 - Math.min(d.health.used_pct ?? 0, 100)} % životnosti
						</span>
						<span class="d-h-item"><Thermometer size={15} /> {d.health.temp_c} °C</span>
						<span class="d-h-item dim">{d.health.power_on_hours} h provozu</span>
					{:else if d.key !== 'orphan'}
						<span class="d-h-item dim">SMART nedostupný (SATA — doplní se)</span>
					{/if}
				</div>
				{#each d.vols as v (v.letter)}
					{@const pct = usedPct(v)}
					<div class="vol-row">
						<span class="vol-letter mono">{v.letter}:</span>
						<span class="vol-label">{v.label || 'Místní disk'}</span>
						<span class="vol-fs label-tech">{v.fs}</span>
						<span class="bar"
							><span class="bar-fill" style:width="{pct}%" style:background={barColor(pct)}
							></span></span
						>
						<span class="vol-nums mono"
							>{fmtSize(v.total_bytes - v.free_bytes)} / {fmtSize(v.total_bytes)} ·
							<b>{fmtSize(v.free_bytes)} volných</b></span
						>
					</div>
				{/each}
			</div>
		{/each}
	</div>

	<!-- ── Úklid: progres indexace + výsledky analýzy ── -->
	<section class="card cleanup">
		<div class="c-head">
			<span class="label-tech">// úklid disků</span>
			{#if cleanup?.indexing?.length && !indexingDone}
				<span class="c-status">
					<Loader size={15} class="spin" />
					indexuji disky —
					{#each cleanup.indexing as [l, n, done, err] (l)}
						<span class="mono" class:err={!!err}>{l}: {done ? '✓' : n.toLocaleString('cs-CZ')}</span>
					{/each}
				</span>
			{:else if cleanup?.running}
				<span class="c-status"><Loader size={15} class="spin" /> analyzuji obsah disků…</span>
			{:else if cleanup?.report}
				<span class="c-status dim">
					analýza hotová · v duplicitách zbytečně ~{fmtSize(dupWaste)}
				</span>
			{:else}
				<span class="c-status dim">služba analýzu spustí sama krátce po startu…</span>
			{/if}
		</div>

		<!-- Svazky, které nešly zindexovat — s důvodem, ne mlčky. -->
		{#each (cleanup?.indexing ?? []).filter((i) => i[3]) as [l, , , err] (l)}
			<p class="idx-err">
				<TriangleAlert size={15} />
				<b>{l}:</b> disk nebylo možné prohledat — {err}
			</p>
		{/each}

		{#if cleanup?.report}
			{@const r = cleanup.report}
			<div class="c-cols">
				<!-- Duplicity -->
				<div class="c-block">
					<h3>
						<Copy size={15} /> Duplicity — {r.dups.length} skupin, {fmtSize(dupWaste)} navíc
					</h3>
					{#if r.dups.length === 0}
						<p class="note">žádné duplicitní soubory (média/archivy/dokumenty ≥ 1 MB)</p>
					{:else}
						<ul class="dup-list">
							{#each r.dups.slice(0, 40) as [size, paths], gi (gi)}
								<li>
									<span class="dup-size mono">{fmtSize(size)} × {paths.length}</span>
									{#each paths as p (p)}
										<div class="row-wrap">
											<button class="row" onclick={() => openPath(p)}>
												<span class="r-path mono">{p}</span>
											</button>
											<button
												class="del-btn"
												title="Přesunout do koše (jde vrátit)"
												onclick={(ev) => askDelete([p], ev)}
											>
												<Trash2 size={14} />
											</button>
										</div>
									{/each}
								</li>
							{/each}
						</ul>
					{/if}
				</div>

				<!-- 0bajtové -->
				<div class="c-block">
					<h3><FileX size={15} /> Prázdné soubory (0 B) — {r.zero_byte.length}</h3>
					{#if r.zero_byte.length === 0}
						<p class="note">žádné prázdné soubory v profilech</p>
					{:else}
						{#each r.zero_byte.slice(0, 60) as p (p)}
							<div class="row-wrap">
								<button class="row" onclick={() => openPath(p)}>
									<span class="r-path mono">{p}</span>
								</button>
								<button
									class="del-btn"
									title="Přesunout do koše (jde vrátit)"
									onclick={(ev) => askDelete([p], ev)}
								>
									<Trash2 size={14} />
								</button>
							</div>
						{/each}
					{/if}
				</div>
			</div>
			<p class="note big">
				Winsent ukáže, co zabírá místo — mazat necháme na tobě. Klik otevře složku
				v Průzkumníku, kde soubor uvidíš v kontextu a smažeš vědomě. Žádná appka
				za tebe nemá rozhodovat, co je tvoje data a co smetí. Když si přesto chceš
				nechat pomoct, ikona koše u položky ji přesune do koše — odkud jde vrátit.
			</p>
		{/if}
	</section>

	<!-- ── Přesun do koše: plán → potvrzení (v8, T1) ── -->
	{#if delPlan}
		<div class="dlg-backdrop" role="presentation" onclick={() => (delPlan = null)} onkeydown={() => {}}>
			<div class="dlg" role="dialog" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
				{#if delPlan.deny}
					<h2>Nelze smazat</h2>
					<p class="d-why">{delPlan.deny.deny_reason}</p>
					<div class="d-actions">
						<button class="d-btn" onclick={() => (delPlan = null)}>Zavřít</button>
					</div>
				{:else}
					<h2>Přesunout do koše?</h2>
					<ul class="d-steps">
						{#each delPlan.plan.steps as s, i (i)}
							<li class:warn={s.description.startsWith('pozor')}>{s.description}</li>
						{/each}
					</ul>
					<p class="d-note">Z koše jde soubor kdykoli vrátit zpět.</p>
					<div class="d-actions">
						<button class="d-btn" onclick={() => (delPlan = null)}>Zrušit</button>
						<button class="d-btn primary" disabled={delBusy} onclick={confirmDelete}>
							{delBusy ? 'mažu…' : 'Do koše'}
						</button>
					</div>
				{/if}
			</div>
		</div>
	{/if}
	{#if delToast}
		<div class="dlg-toast {delToast.kind}">{delToast.text}</div>
	{/if}

	<!-- ── Co zabírá nejvíc místa ── -->
	{#if bigVolumes.length}
		<section class="card">
			<div class="c-head">
				<span class="label-tech">// co zabírá nejvíc místa</span>
				<div class="vol-seg">
					{#each bigVolumes as l (l)}
						<button class:active={bigShown === l} onclick={() => (bigVolume = l)}>{l}:</button>
					{/each}
				</div>
			</div>
			<div class="c-cols">
				<div class="c-block tall">
					<h3><FolderTree size={15} /> Největší složky <em>{bigDirs.length}</em></h3>
					{#each bigDirs as [, path, size] (path)}
						<button class="row" onclick={() => openPath(path)} title="Otevřít v Průzkumníku">
							<span class="r-path mono">{path}</span>
							{#if isSystemPath(path)}<SystemBadge compact />{/if}
							<span class="r-size mono">{fmtSize(size)}</span>
						</button>
					{/each}
				</div>
				<div class="c-block tall">
					<h3><FileType2 size={15} /> Největší soubory <em>{bigFiles.length}</em></h3>
					{#each bigFiles as [, path, size] (path)}
						<button class="row" onclick={() => openPath(path)} title="Otevřít v Průzkumníku">
							<span class="r-path mono">{path}</span>
							{#if isSystemPath(path)}<SystemBadge compact />{/if}
							<span class="r-size mono">{fmtSize(size)}</span>
						</button>
					{/each}
				</div>
			</div>
		</section>
	{/if}
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: 14px;
		height: 100%;
		min-height: 0;
		overflow-y: auto;
	}
	.head {
		display: flex;
		align-items: baseline;
		gap: 12px;
	}
	.head h1 {
		font-size: 1.2rem;
		font-weight: 600;
	}
	.sub {
		color: var(--text-faint);
		font-size: 0.84rem;
	}
	.card {
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 14px 16px;
	}

	/* Karty disků — plná šířka, responsivní grid. */
	.disk-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(420px, 1fr));
		gap: 10px;
	}
	.disk .d-head {
		display: flex;
		align-items: center;
		gap: 14px;
		flex-wrap: wrap;
		margin-bottom: 10px;
	}
	.d-model {
		font-weight: 600;
		font-size: 0.95rem;
	}
	.d-health,
	.d-h-item {
		display: flex;
		align-items: center;
		gap: 6px;
		font-family: var(--font-mono);
		font-size: 0.82rem;
	}
	.vol-row {
		display: grid;
		grid-template-columns: 34px minmax(90px, auto) auto 1fr minmax(230px, auto);
		gap: 12px;
		align-items: center;
		padding: 6px 0;
	}
	.vol-letter {
		font-weight: 500;
	}
	.vol-label {
		font-size: 0.86rem;
		color: var(--text-dim);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.vol-fs {
		font-size: 0.68rem;
	}
	.bar {
		height: 7px;
		border-radius: 4px;
		background: var(--surface-hover);
		overflow: hidden;
		display: block;
	}
	.bar-fill {
		display: block;
		height: 100%;
		border-radius: 4px;
	}
	.vol-nums {
		font-size: 0.78rem;
		color: var(--text-dim);
		text-align: right;
	}
	.vol-nums b {
		color: var(--text);
		font-weight: 500;
	}

	/* Úklid */
	.c-head {
		display: flex;
		align-items: center;
		gap: 14px;
		flex-wrap: wrap;
	}
	.c-status {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 0.86rem;
		color: var(--text-dim);
	}
	.vol-seg {
		display: flex;
		gap: 2px;
		margin-left: auto;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 2px;
	}
	.vol-seg button {
		background: none;
		border: none;
		color: var(--text-dim);
		font: inherit;
		font-family: var(--font-mono);
		font-size: 0.76rem;
		padding: 3px 10px;
		border-radius: 3px;
		cursor: pointer;
	}
	.vol-seg button.active {
		background: var(--surface-hover);
		color: var(--text);
	}
	:global(.spin) {
		animation: spin 1.1s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
	.c-cols {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(340px, 1fr));
		gap: 14px;
		margin-top: 12px;
	}
	.c-block h3 {
		display: flex;
		align-items: center;
		gap: 7px;
		font-size: 0.82rem;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		color: var(--text-dim);
		font-weight: 500;
		margin: 0 0 8px;
	}
	.c-block.tall {
		max-height: 60vh;
	}
	.c-block {
		min-width: 0;
		max-height: 44vh;
		overflow-y: auto;
		scrollbar-gutter: stable;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--panel);
		padding: 10px 12px;
	}
	.dup-list {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.dup-list li {
		border-bottom: 1px dashed var(--border);
		padding: 5px 0;
	}
	.dup-size {
		font-size: 0.76rem;
		color: var(--warn);
	}
	.row {
		width: 100%;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 4px 2px;
		background: none;
		border: none;
		color: var(--text);
		font: inherit;
		cursor: pointer;
		text-align: left;
	}
	.row:hover {
		background: var(--surface-hover);
	}
	.r-path {
		flex: 1;
		font-size: 0.8rem;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		direction: rtl;
		text-align: left;
	}
	.r-size {
		font-size: 0.78rem;
		color: var(--text-dim);
	}
	.note {
		font-size: 0.74rem;
		color: var(--text-faint);
		margin: 6px 0 0;
	}
	.c-block h3 em {
		font-style: normal;
		font-family: var(--font-mono);
		font-size: 0.68rem;
		color: var(--text-faint);
	}
	.idx-err {
		display: flex;
		align-items: center;
		gap: 8px;
		margin: 10px 0 0;
		font-size: 0.82rem;
		color: var(--warn);
	}
	.mono.err {
		color: var(--warn);
	}
	.note.big {
		font-size: 0.8rem;
		margin-top: 12px;
	}

	/* Hledání */
	.s-head {
		display: flex;
		align-items: center;
		gap: 10px;
		color: var(--text-dim);
	}
	.s-head input {
		flex: 1;
		background: none;
		border: none;
		outline: none;
		color: var(--text);
		font: inherit;
		font-size: 0.95rem;
		padding: 4px 0;
	}
	.sel {
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text);
		font: inherit;
		font-size: 0.82rem;
		padding: 4px 8px;
	}
	.hits {
		list-style: none;
		margin: 10px 0 0;
		padding: 0;
		max-height: 38vh;
		overflow-y: auto;
	}
	.hits .row {
		border-bottom: 1px dashed var(--border);
	}
	.row.system-f .r-path {
		color: var(--warn);
	}
	.row.hidden-f .r-path {
		color: var(--text-faint);
	}
	/* Řádek s cestou + tlačítko koše (mazání je nabídka, ne
	   hlavní cesta — ta zůstává „otevřít složku"). */
	.row-wrap {
		display: flex;
		align-items: center;
		gap: 4px;
	}
	.row-wrap .row {
		flex: 1;
		min-width: 0;
	}
	.del-btn {
		flex: none;
		background: none;
		border: none;
		color: var(--text-faint);
		padding: 3px 5px;
		border-radius: var(--radius-sm);
		cursor: pointer;
		display: grid;
		place-items: center;
	}
	.del-btn:hover {
		color: var(--danger);
		background: var(--surface-hover);
	}

	/* Dialog potvrzení (stejný jazyk jako v Tasks) */
	.dlg-backdrop {
		position: fixed;
		inset: 0;
		background: rgba(8, 9, 11, 0.62);
		display: grid;
		place-items: center;
		z-index: 40;
	}
	.dlg {
		width: min(560px, 90vw);
		background: #16171c;
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-lg);
		padding: 20px 22px;
	}
	.dlg h2 {
		font-size: 1.05rem;
		margin-bottom: 10px;
	}
	.d-steps {
		list-style: none;
		margin: 0 0 12px;
		padding: 0;
		max-height: 40vh;
		overflow-y: auto;
		font-size: 0.84rem;
	}
	.d-steps li {
		padding: 5px 0;
		border-bottom: 1px dashed var(--border);
		word-break: break-all;
	}
	.d-steps li.warn {
		color: var(--warn);
	}
	.d-why {
		color: var(--danger);
		font-size: 0.88rem;
		margin-bottom: 12px;
	}
	.d-note {
		font-size: 0.78rem;
		color: var(--text-faint);
		margin-bottom: 14px;
	}
	.d-actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
	}
	.d-btn {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text);
		font: inherit;
		font-size: 0.84rem;
		padding: 7px 14px;
		cursor: pointer;
	}
	.d-btn.primary {
		border-color: color-mix(in srgb, var(--warn) 50%, transparent);
		color: var(--warn);
	}
	.d-btn:hover {
		border-color: var(--border-strong);
	}
	.dlg-toast {
		position: fixed;
		bottom: 18px;
		left: 50%;
		transform: translateX(-50%);
		padding: 9px 16px;
		border-radius: var(--radius);
		background: #16171c;
		border: 1px solid var(--border-strong);
		font-size: 0.84rem;
		z-index: 50;
	}
	.dlg-toast.ok {
		color: var(--ok);
	}
	.dlg-toast.deny {
		color: var(--danger);
	}

	.mono {
		font-family: var(--font-mono);
	}
	.dim {
		color: var(--text-faint);
	}
	.empty {
		color: var(--text-faint);
		font-size: 0.88rem;
		padding: 12px;
	}
</style>
