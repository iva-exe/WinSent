<script>
	// Incidenty (v3, SPEC kap. 16): záseky, pády aplikací a BSOD pod
	// jedním modelem. Seznam + detail s křivkou okna T-5min..T+30s.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { TriangleAlert, Zap, MonitorX, Timer, RefreshCw, Trash2 } from 'lucide-svelte';
	import Sparkline from '$lib/Sparkline.svelte';

	let incidents = $state([]);
	let selected = $state(null);
	let windowPoints = $state([]);
	let loadError = $state('');

	// Druh incidentu → vizuál.
	const kinds = {
		stall: { label: 'Zásek systému', icon: Timer, color: 'var(--warn)' },
		app_crash: { label: 'Pád aplikace', icon: Zap, color: 'var(--danger)' },
		bsod: { label: 'BSOD / tvrdý pád', icon: MonitorX, color: 'var(--danger)' }
	};
	const kindOf = (k) => kinds[k] ?? { label: k, icon: TriangleAlert, color: 'var(--warn)' };

	// Lidský popis příčiny záseku.
	const causes = {
		paging: 'paging — nedostatek RAM (hard faulty)',
		io: 'saturace disku (fronta / latence)',
		thermal: 'teplotní omezení CPU',
		cpu: 'saturace CPU',
		unknown: 'neznámá příčina (typicky ovladač/DPC)'
	};

	function parseDetail(i) {
		try {
			return JSON.parse(i.detail ?? '{}');
		} catch {
			return {};
		}
	}

	function fmtTs(ts) {
		const d = new Date(ts * 1000);
		return d.toLocaleDateString('cs-CZ') + ' ' + d.toLocaleTimeString('cs-CZ');
	}

	async function load() {
		try {
			incidents = await invoke('query_incidents', { limit: 100 });
			loadError = '';
			if (selected && !incidents.some((i) => i.id === selected.id)) selected = null;
		} catch (e) {
			loadError = String(e);
		}
	}

	// Smazání ZÁZNAMU (jen náš seznam — nic v systému se nemění).
	async function remove(i, ev) {
		ev?.stopPropagation();
		try {
			await invoke('delete_incident', { id: i.id });
			if (selected?.id === i.id) selected = null;
			load();
		} catch {
			/* služba mimo */
		}
	}

	// Křivka systému v okně incidentu (CPU %) — z retenční kaskády,
	// takže funguje i pro starší incidenty (řidší body).
	async function select(i) {
		selected = i;
		windowPoints = [];
		const from = i.window_from ?? i.ts - 300;
		const to = i.window_to ?? i.ts + 30;
		try {
			windowPoints = await invoke('query_system_history', { from, to });
		} catch {
			windowPoints = [];
		}
	}

	// Index bodu nejblíž okamžiku incidentu (marker ve sparkline).
	let markerIdx = $derived.by(() => {
		if (!selected || windowPoints.length === 0) return null;
		let best = 0;
		for (let k = 1; k < windowPoints.length; k++) {
			if (Math.abs(windowPoints[k].ts - selected.ts) < Math.abs(windowPoints[best].ts - selected.ts))
				best = k;
		}
		return best;
	});

	onMount(() => {
		load();
		const t = setInterval(load, 15000);
		return () => clearInterval(t);
	});
</script>

<div class="page">
	<header class="head">
		<h1>Incidenty</h1>
		<span class="sub">záseky · pády aplikací · BSOD — s nahranou časovou osou</span>
		<button class="refresh" onclick={load} title="Obnovit"><RefreshCw size={15} /></button>
	</header>

	{#if loadError}
		<p class="empty">Nelze načíst incidenty: {loadError}</p>
	{:else if incidents.length === 0}
		<div class="empty explain">
			<p><b>Zatím žádné incidenty — to je dobře.</b></p>
			<p>
				Winsent na pozadí nepřetržitě nahrává stav systému. Když se něco pokazí, objeví se
				to tady i s kontextem, který jinde nenajdeš:
			</p>
			<p>
				<Timer size={14} /> <b>Zásek systému</b> — celé PC přestane reagovat; hlídač to změří
				a určí viníka (disk / RAM / CPU / přehřátí).<br />
				<Zap size={14} /> <b>Pád aplikace</b> — proces skončil chybou; uvidíš exit kód
				a co se dělo 5 minut předtím.<br />
				<MonitorX size={14} /> <b>Modrá obrazovka</b> — po restartu se přečte minidump
				a bugcheck se přeloží do lidské řeči.
			</p>
		</div>
	{:else}
		<div class="cols">
			<ul class="list">
				{#each incidents as i (i.id)}
					{@const k = kindOf(i.kind)}
					<li>
						<button class="row" class:active={selected?.id === i.id} onclick={() => select(i)}>
							<span class="kind-ico" style:color={k.color}><k.icon size={16} /></span>
							<span class="row-main">
								<span class="row-title">{k.label}</span>
								<span class="row-culprit">{i.culprit ?? '—'}</span>
							</span>
							<span class="row-ts">{fmtTs(i.ts)}</span>
							<span
								class="row-del"
								role="button"
								tabindex="-1"
								title="Odstranit záznam (v systému se nic nemění)"
								onclick={(ev) => remove(i, ev)}
								onkeydown={() => {}}><Trash2 size={15} /></span
							>
						</button>
					</li>
				{/each}
			</ul>

			<section class="detail">
				{#if !selected}
					<p class="empty">Vyber incident vlevo.</p>
				{:else}
					{@const k = kindOf(selected.kind)}
					{@const d = parseDetail(selected)}
					<div class="d-head">
						<span class="kind-ico big" style:color={k.color}><k.icon size={21} /></span>
						<div>
							<h2>{k.label}</h2>
							<span class="d-ts">{fmtTs(selected.ts)}</span>
						</div>
					</div>

					<div class="d-grid">
						<div class="d-item wide">
							<span class="d-label">Viník</span>
							<span class="d-value">{selected.culprit ?? 'nezjištěn'}</span>
						</div>
						{#if selected.kind === 'stall'}
							<div class="d-item">
								<span class="d-label">Výpadek</span>
								<span class="d-value mono">{d.lag_ms ?? '—'} ms</span>
							</div>
							<div class="d-item wide">
								<span class="d-label">Příčina</span>
								<span class="d-value">{causes[d.cause] ?? d.cause ?? '—'}</span>
							</div>
						{/if}
						{#if selected.kind === 'app_crash'}
							<div class="d-item">
								<span class="d-label">Exit kód</span>
								<span class="d-value mono"
									>0x{(d.exit_code ?? 0).toString(16).toUpperCase().padStart(8, '0')}</span
								>
							</div>
							<div class="d-item">
								<span class="d-label">Proces</span>
								<span class="d-value mono">{d.name || '—'}</span>
							</div>
						{/if}
						{#if selected.kind === 'bsod'}
							<div class="d-item">
								<span class="d-label">Bugcheck</span>
								<span class="d-value mono"
									>{d.bugcheck != null
										? '0x' + d.bugcheck.toString(16).toUpperCase().padStart(8, '0')
										: '—'}</span
								>
							</div>
							{#if d.dump}
								<div class="d-item wide">
									<span class="d-label">Minidump</span>
									<span class="d-value mono small">{d.dump}</span>
								</div>
							{/if}
						{/if}
						{#if selected.etl_path}
							<div class="d-item wide">
								<span class="d-label">Černá skříňka</span>
								<span class="d-value mono small">{selected.etl_path}</span>
							</div>
						{/if}
					</div>

					{#if d.top?.length}
						<h3 class="sec">Top procesy v okně</h3>
						<ul class="top">
							{#each d.top as t (t.pid)}
								<li>
									<span class="mono dim">{t.pid}</span>
									<span>{t.name || '(bez jména)'}</span>
									<span class="mono">{t.value}</span>
								</li>
							{/each}
						</ul>
					{/if}

					<h3 class="sec">Co se dělo okolo (okno incidentu)</h3>
					{#if windowPoints.length > 1}
						{@const maxMem = Math.max(...windowPoints.map((p) => p.mem_used_mb), 1)}
						{@const maxNet = Math.max(
							...windowPoints.map((p) => p.net_rx_bps + p.net_tx_bps),
							1
						)}
						<div class="spark">
							<span class="spark-l">CPU</span>
							<Sparkline
								values={windowPoints.map((p) => p.cpu_pct)}
								height={48}
								marker={markerIdx}
							/>
						</div>
						<div class="spark">
							<span class="spark-l">RAM</span>
							<Sparkline
								values={windowPoints.map((p) => (p.mem_used_mb / maxMem) * 100)}
								height={48}
								marker={markerIdx}
							/>
						</div>
						<div class="spark">
							<span class="spark-l">Síť</span>
							<Sparkline
								values={windowPoints.map(
									(p) => ((p.net_rx_bps + p.net_tx_bps) / maxNet) * 100
								)}
								height={48}
								marker={markerIdx}
							/>
						</div>
						<div class="spark-range">
							<span>{fmtTs(windowPoints[0].ts)}</span>
							<span>linka = okamžik incidentu</span>
							<span>{fmtTs(windowPoints[windowPoints.length - 1].ts)}</span>
						</div>
					{:else}
						<p class="empty small">Pro toto okno už nejsou vzorky v historii.</p>
					{/if}
					<p class="foot">
						Záznam jde odstranit ikonou koše v seznamu — maže se jen tenhle zápis,
						v systému se nic nemění.
					</p>
				{/if}
			</section>
		</div>
	{/if}
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
		letter-spacing: 0.02em;
	}
	.sub {
		color: var(--text-faint);
		font-size: 0.78rem;
	}
	.refresh {
		margin-left: auto;
		background: none;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text-dim);
		padding: 4px 7px;
		cursor: pointer;
		display: grid;
		place-items: center;
	}
	.refresh:hover {
		color: var(--text);
		border-color: var(--border-strong);
	}

	.cols {
		display: grid;
		grid-template-columns: minmax(360px, 480px) 1fr;
		gap: 14px;
		min-height: 0;
		flex: 1;
	}
	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		overflow-y: auto;
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		background: var(--surface);
	}
	.row {
		width: 100%;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 9px 12px;
		background: none;
		border: none;
		border-bottom: 1px dashed var(--border);
		color: var(--text);
		cursor: pointer;
		text-align: left;
		font: inherit;
	}
	.row:hover {
		background: var(--surface-hover);
	}
	.row.active {
		background: var(--surface-hover);
		box-shadow: inset 2px 0 0 var(--accent);
	}
	.kind-ico {
		display: grid;
		place-items: center;
		filter: drop-shadow(0 0 5px color-mix(in srgb, currentColor 60%, transparent));
	}
	.kind-ico.big {
		filter: drop-shadow(0 0 8px color-mix(in srgb, currentColor 65%, transparent));
	}
	.row-main {
		display: flex;
		flex-direction: column;
		min-width: 0;
		flex: 1;
	}
	.row-title {
		font-size: 0.85rem;
		font-weight: 500;
	}
	.row-culprit {
		font-size: 0.75rem;
		color: var(--text-dim);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.row-ts {
		font-family: var(--font-mono);
		font-size: 0.68rem;
		color: var(--text-faint);
		white-space: nowrap;
	}

	.detail {
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 16px;
		overflow-y: auto;
		min-height: 0;
	}
	.d-head {
		display: flex;
		align-items: center;
		gap: 12px;
		margin-bottom: 14px;
	}
	.d-head h2 {
		font-size: 1rem;
		font-weight: 600;
	}
	.d-ts {
		font-family: var(--font-mono);
		font-size: 0.72rem;
		color: var(--text-dim);
	}
	.d-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 8px;
	}
	.d-item {
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 8px 10px;
		display: flex;
		flex-direction: column;
		gap: 3px;
		min-width: 0;
	}
	.d-item.wide {
		grid-column: 1 / -1;
	}
	.d-label {
		font-size: 0.66rem;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--text-faint);
	}
	.d-value {
		font-size: 0.86rem;
	}
	.d-value.small {
		font-size: 0.72rem;
		word-break: break-all;
	}
	.mono {
		font-family: var(--font-mono);
	}
	.dim {
		color: var(--text-faint);
	}
	.sec {
		margin: 16px 0 8px;
		font-size: 0.72rem;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--text-dim);
		font-weight: 500;
	}
	.top {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.top li {
		display: grid;
		grid-template-columns: 64px 1fr auto;
		gap: 10px;
		font-size: 0.8rem;
		padding: 5px 10px;
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
	}
	.spark {
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--panel);
		padding: 8px 10px 4px;
		margin-bottom: 6px;
		position: relative;
	}
	.spark-l {
		position: absolute;
		top: 5px;
		left: 9px;
		font-size: 0.64rem;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--text-faint);
		z-index: 1;
	}
	.spark-range {
		display: flex;
		justify-content: space-between;
		font-family: var(--font-mono);
		font-size: 0.68rem;
		color: var(--text-faint);
		margin-top: 4px;
	}
	.row-del {
		display: grid;
		place-items: center;
		color: var(--text-faint);
		padding: 3px;
		border-radius: var(--radius-sm);
	}
	.row-del:hover {
		color: var(--danger);
		background: var(--surface-hover);
	}
	.empty.explain {
		max-width: 620px;
		line-height: 1.55;
	}
	.empty.explain p {
		margin: 0 0 10px;
	}
	.foot {
		margin-top: 14px;
		font-size: 0.76rem;
		color: var(--text-faint);
	}
	.empty {
		color: var(--text-faint);
		font-size: 0.85rem;
		padding: 18px;
	}
	.empty.small {
		padding: 8px 0;
	}
</style>
