<script>
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { Tween } from 'svelte/motion';
	import { cubicOut } from 'svelte/easing';
	import { flip } from 'svelte/animate';
	import { daemon } from '$lib/daemon.svelte.js';
	import LiveChart from '$lib/LiveChart.svelte';

	// Buffery na celou dostupnou historii (1 h retence surových vzorků).
	const CAP = 3600;

	let ts = $state([]);
	let cpu = $state([]);
	let mem = $state([]);
	let sys = $state([]);
	let down = $state([]);
	let up = $state([]);

	let system = $state(null);
	let procs = $state([]);
	let error = $state('');
	let historyLoaded = false;

	// Metrika grafu — vždy jedna čára.
	let mode = $state('sys');
	const modes = [
		{ id: 'cpu', label: 'CPU' },
		{ id: 'ram', label: 'RAM' },
		{ id: 'sys', label: 'System' },
		{ id: 'down', label: '↓ Down' },
		{ id: 'up', label: '↑ Up' }
	];
	const chartValues = $derived(
		mode === 'cpu' ? cpu : mode === 'ram' ? mem : mode === 'down' ? down : mode === 'up' ? up : sys
	);

	// Zátěž systému: základ je průměr komponent, ale čím vyšší je
	// nejvytíženější komponenta, tím víc výsledek táhne k jejímu maximu
	// (CPU 100 % + RAM 20 % ⇒ 100 %; CPU 20 % + RAM 60 % ⇒ ~52 %).
	function sysLoad(components) {
		const vals = components.filter((v) => v != null);
		if (!vals.length) return 0;
		const mean = vals.reduce((a, b) => a + b, 0) / vals.length;
		const max = Math.max(...vals);
		const w = Math.min(max / 100, 1);
		return mean * (1 - w) + max * w;
	}

	// Hover a zámek času (klik do grafu). Zámek má přednost před hoverem.
	let hover = $state(null);
	let pinned = $state(null);
	const focusTs = $derived(pinned ?? hover?.t ?? null);
	const focusIdx = $derived(
		focusTs == null ? null : ts.findLastIndex((t) => t <= focusTs)
	);

	// Stav tasků z minulosti (hover/zámek mimo přítomnost).
	let histProcs = $state(null); // { ts, rows } | null
	let histTimer = null;

	const showingPast = $derived(
		focusTs != null && ts.length > 0 && focusTs < ts[ts.length - 1] - 2
	);

	$effect(() => {
		// Debounce dotazu na historii tasků podle fokusu.
		const t = focusTs;
		const past = showingPast;
		clearTimeout(histTimer);
		if (!past || t == null) {
			histProcs = null;
			return;
		}
		histTimer = setTimeout(async () => {
			try {
				histProcs = await invoke('query_procs_at', { ts: Math.round(t) });
			} catch {
				histProcs = null;
			}
		}, 200);
	});

	// Tweenované readouty — čísla plynou místo skoků 1×/s.
	const cpuT = new Tween(0, { duration: 700, easing: cubicOut });
	const ramT = new Tween(0, { duration: 700, easing: cubicOut });
	const sysT = new Tween(0, { duration: 700, easing: cubicOut });

	// Řazení tabulky.
	let sortKey = $state('cpu_pct');
	let sortDir = $state(-1);

	// Zdroj tabulky: minulost (hover/zámek) nebo živá data; každý řádek
	// dostane vlastní zátěž `sys` stejným vzorcem jako systém.
	const tableRows = $derived.by(() => {
		const total = (system?.mem_total_mb ?? 0) * 1024 * 1024;
		const src = histProcs ? histProcs.rows : procs;
		return src.map((p) => ({
			...p,
			threads: p.threads ?? null,
			sys_pct: sysLoad([p.cpu_pct, total > 0 ? (p.ws_bytes / total) * 100 : null])
		}));
	});

	const sorted = $derived(
		[...tableRows].sort((a, b) => {
			const va = a[sortKey];
			const vb = b[sortKey];
			const cmp = typeof va === 'string' ? va.localeCompare(vb) : (va ?? -1) - (vb ?? -1);
			return cmp * sortDir;
		})
	);

	function setSort(key) {
		if (sortKey === key) {
			sortDir = -sortDir;
		} else {
			sortKey = key;
			sortDir = key === 'name' ? 1 : -1;
		}
	}

	function push(arr, v) {
		return [...arr.slice(-(CAP - 1)), v];
	}

	async function pollSystem() {
		try {
			const s = await invoke('query_system');
			system = s;
			error = '';
			const memPct = (s.mem_used_mb / Math.max(s.mem_total_mb, 1)) * 100;
			const sysPct = sysLoad([s.cpu_pct, memPct]);
			cpuT.set(s.cpu_pct);
			ramT.set(s.mem_used_mb / 1024);
			sysT.set(sysPct);
			const now = Math.floor(Date.now() / 1000);
			ts = push(ts, now);
			cpu = push(cpu, s.cpu_pct);
			mem = push(mem, memPct);
			sys = push(sys, sysPct);
			down = push(down, s.net_rx_bps);
			up = push(up, s.net_tx_bps);

			if (!historyLoaded) {
				historyLoaded = true;
				loadHistory(s, now);
			}
		} catch (e) {
			system = null;
			error = String(e);
		}
	}

	// Jednorázově napřed doplní hodinovou historii ze služby (SQLite),
	// aby šlo grafem scrollovat do minulosti hned po otevření.
	async function loadHistory(s, now) {
		try {
			const points = await invoke('query_system_history', { from: now - CAP, to: now - 1 });
			if (!points.length) return;
			const total = Math.max(s.mem_total_mb, 1);
			const hTs = [], hCpu = [], hMem = [], hSys = [], hDown = [], hUp = [];
			for (const p of points) {
				const memPct = (p.mem_used_mb / total) * 100;
				hTs.push(p.ts);
				hCpu.push(p.cpu_pct);
				hMem.push(memPct);
				hSys.push(sysLoad([p.cpu_pct, memPct]));
				hDown.push(p.net_rx_bps ?? 0);
				hUp.push(p.net_tx_bps ?? 0);
			}
			// Prepend před živé body (bez duplicit na hranici).
			const cut = ts.length && hTs.length ? hTs.findIndex((t) => t >= ts[0]) : -1;
			const end = cut === -1 ? hTs.length : cut;
			ts = [...hTs.slice(0, end), ...ts];
			cpu = [...hCpu.slice(0, end), ...cpu];
			mem = [...hMem.slice(0, end), ...mem];
			sys = [...hSys.slice(0, end), ...sys];
			down = [...hDown.slice(0, end), ...down];
			up = [...hUp.slice(0, end), ...up];
		} catch {
			// historie není fatální — graf pojede od teď
		}
	}

	async function pollProcs() {
		try {
			procs = await invoke('query_procs');
		} catch {
			procs = [];
		}
	}

	function fmtMem(bytes) {
		const mb = bytes / (1024 * 1024);
		return mb >= 1024 ? `${(mb / 1024).toFixed(2)} GB` : `${mb.toFixed(1)} MB`;
	}
	function fmtBps(v) {
		if (v == null) return '—';
		const mb = v / (1024 * 1024);
		return mb >= 1 ? `${mb.toFixed(1)} MB/s` : `${(v / 1024).toFixed(0)} kB/s`;
	}
	function fmtClock(unix) {
		return new Date(unix * 1000).toLocaleTimeString('cs-CZ');
	}

	// Klik mimo kartu grafu ruší zámek času.
	function onWindowClick(e) {
		if (pinned != null && !e.target.closest('.chart-card')) {
			pinned = null;
		}
	}

	onMount(() => {
		pollSystem();
		pollProcs();
		const t = setInterval(() => {
			pollSystem();
			pollProcs();
		}, 1000);
		return () => clearInterval(t);
	});

	const arrow = $derived(sortDir === -1 ? '↓' : '↑');
</script>

<svelte:window onclick={onWindowClick} />

<div class="tasks">
	<!-- ── Hlavní časový graf ── -->
	<section class="card chart-card">
		<header class="card-head">
			<div class="head-left">
				<span class="label-tech">// tasks / system</span>
				<div class="seg">
					{#each modes as m (m.id)}
						<button class:active={mode === m.id} onclick={() => (mode = m.id)}>
							{m.label}
						</button>
					{/each}
				</div>
			</div>
			<div class="readouts value-mono">
				{#if focusIdx != null}
					<!-- Hodnoty z vybraného času (zámek/hover) -->
					<span class="readout"><span class="k">{pinned != null ? '⌖ ČAS' : 'ČAS'}</span><span class="v">{fmtClock(ts[focusIdx])}</span></span>
					<span class="readout"><span class="k">CPU</span><span class="v">{cpu[focusIdx]?.toFixed(1) ?? '—'} %</span></span>
					<span class="readout"><span class="k">RAM</span><span class="v">{mem[focusIdx]?.toFixed(1) ?? '—'} %</span></span>
					<span class="readout"><span class="k">SYS</span><span class="v accent">{sys[focusIdx]?.toFixed(1) ?? '—'} %</span></span>
					<span class="readout"><span class="k">↓</span><span class="v">{fmtBps(down[focusIdx])}</span></span>
					<span class="readout"><span class="k">↑</span><span class="v">{fmtBps(up[focusIdx])}</span></span>
				{:else if system}
					<span class="readout"><span class="k">SYS</span><span class="v accent">{sysT.current.toFixed(1)} %</span></span>
					<span class="readout"><span class="k">CPU</span><span class="v">{cpuT.current.toFixed(1)} %</span></span>
					<span class="readout"><span class="k">RAM</span><span class="v">{ramT.current.toFixed(1)} / {(system.mem_total_mb / 1024).toFixed(1)} GB</span></span>
					<span class="readout"><span class="k">↓</span><span class="v">{fmtBps(system.net_rx_bps)}</span></span>
					<span class="readout"><span class="k">↑</span><span class="v">{fmtBps(system.net_tx_bps)}</span></span>
					<span class="readout" title="Počet běžících procesů"><span class="k">PROCESY</span><span class="v">{system.proc_count}</span></span>
				{:else}
					<span class="readout"><span class="k">—</span></span>
				{/if}
			</div>
		</header>
		{#if daemon.alive || ts.length > 0}
			<LiveChart
				{ts}
				values={chartValues}
				{mode}
				{pinned}
				onhover={(h) => (hover = h)}
				onpin={(t) => (pinned = t)}
			/>
		{:else}
			<p class="err">{error || 'služba neběží — graf čeká na data'}</p>
		{/if}
	</section>

	<!-- ── Tabulka procesů ── -->
	<section class="card table-card">
		<header class="card-head">
			<span class="label-tech">// processes</span>
			{#if histProcs}
				<span class="label-tech past-badge">
					● stav z {fmtClock(histProcs.ts)} — {pinned != null
						? 'zámek zrušíš klikem mimo graf'
						: 'sjeď myší z grafu pro živá data'}
				</span>
			{/if}
		</header>
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<th class="t-name" onclick={() => setSort('name')}>
							Proces {#if sortKey === 'name'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('pid')}>
							PID {#if sortKey === 'pid'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('sys_pct')}>
							Sys {#if sortKey === 'sys_pct'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('cpu_pct')}>
							CPU {#if sortKey === 'cpu_pct'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('ws_bytes')}>
							Paměť {#if sortKey === 'ws_bytes'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('threads')}>
							Vlákna {#if sortKey === 'threads'}{arrow}{/if}
						</th>
					</tr>
				</thead>
				<tbody>
					{#each sorted as p (p.pid)}
						<tr animate:flip={{ duration: 260 }}>
							<td class="t-name">{p.name}</td>
							<td class="t-num value-mono">{p.pid}</td>
							<td class="t-num value-mono">{p.sys_pct.toFixed(1)} %</td>
							<td class="t-num value-mono">{p.cpu_pct.toFixed(1)} %</td>
							<td class="t-num value-mono">{fmtMem(p.ws_bytes)}</td>
							<td class="t-num value-mono">{p.threads ?? '—'}</td>
						</tr>
					{:else}
						<tr>
							<td colspan="6" class="empty label-tech">
								{daemon.alive ? 'čekám na první vzorek…' : 'služba neběží'}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>
</div>

<style>
	.tasks {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		height: 100%;
		min-height: 0;
	}

	.card {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 0.9rem 1rem;
	}
	.card-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 0.7rem;
		min-height: 26px;
	}
	.head-left {
		display: flex;
		align-items: center;
		gap: 1rem;
	}

	.seg {
		display: inline-flex;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}
	.seg button {
		border: 0;
		background: transparent;
		color: var(--text-faint);
		font-family: var(--font-mono);
		font-size: 10.5px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		padding: 0.28rem 0.7rem;
		cursor: default;
		transition: background 130ms ease-out, color 130ms ease-out;
	}
	.seg button:hover {
		color: var(--text-dim);
		background: var(--surface);
	}
	.seg button.active {
		color: var(--accent);
		background: var(--surface-hover);
	}

	.readouts {
		display: flex;
		gap: 1.1rem;
		font-size: 12px;
		flex-wrap: wrap;
		justify-content: flex-end;
	}
	.readout .k {
		color: var(--text-faint);
		margin-right: 0.4rem;
		font-size: 10.5px;
	}
	.readout .v {
		color: var(--text-dim);
	}
	.readout .v.accent {
		color: var(--accent);
	}

	.err {
		margin: 0.6rem 0;
		color: var(--danger);
		font-size: 0.85rem;
	}

	.past-badge {
		color: var(--warn);
	}

	/* ── tabulka ── */
	.table-card {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-height: 0;
		padding-bottom: 0;
	}
	.table-wrap {
		flex: 1;
		overflow-y: auto;
		min-height: 0;
		margin: 0 -1rem;
	}
	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.86rem;
	}
	thead th {
		position: sticky;
		top: 0;
		background: rgba(26, 27, 33, 0.92);
		backdrop-filter: blur(8px);
		text-align: left;
		font-family: var(--font-mono);
		font-size: 10.5px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		font-weight: 400;
		color: var(--text-faint);
		padding: 0.45rem 1rem;
		border-bottom: 1px dotted var(--border-strong);
		white-space: nowrap;
		transition: color 130ms ease-out;
	}
	thead th:hover {
		color: var(--text-dim);
	}
	td {
		padding: 0.34rem 1rem;
		border-bottom: 1px solid var(--border);
		color: var(--text-dim);
		white-space: nowrap;
		transition: background 130ms ease-out;
	}
	tbody tr:hover td {
		background: var(--surface);
		color: var(--text);
	}
	.t-name {
		width: 100%;
		max-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	td.t-name {
		color: var(--text);
	}
	.t-num {
		text-align: right;
	}
	th.t-num {
		text-align: right;
	}
	.empty {
		text-align: center;
		padding: 2rem 0;
	}
</style>
