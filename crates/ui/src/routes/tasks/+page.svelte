<script>
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { daemon } from '$lib/daemon.svelte.js';
	import LiveChart from '$lib/LiveChart.svelte';

	// Rolling okno hlavního grafu: 3 minuty po sekundě.
	const WINDOW = 180;

	let ts = $state([]);
	let cpu = $state([]);
	let mem = $state([]);
	let system = $state(null);
	let procs = $state([]);
	let error = $state('');

	// Řazení tabulky — default CPU sestupně.
	let sortKey = $state('cpu_pct');
	let sortDir = $state(-1);

	const sorted = $derived(
		[...procs].sort((a, b) => {
			const va = a[sortKey];
			const vb = b[sortKey];
			const cmp = typeof va === 'string' ? va.localeCompare(vb) : va - vb;
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

	async function pollSystem() {
		try {
			const s = await invoke('query_system');
			system = s;
			error = '';
			const now = Math.floor(Date.now() / 1000);
			ts = [...ts.slice(-(WINDOW - 1)), now];
			cpu = [...cpu.slice(-(WINDOW - 1)), s.cpu_pct];
			mem = [...mem.slice(-(WINDOW - 1)), (s.mem_used_mb / Math.max(s.mem_total_mb, 1)) * 100];
		} catch (e) {
			system = null;
			error = String(e);
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

<div class="tasks">
	<!-- ── Hlavní časový graf (komponentový princip: graf nahoře) ── -->
	<section class="card">
		<header class="card-head">
			<span class="label-tech">// tasks / system</span>
			<div class="readouts value-mono">
				{#if system}
					<span class="readout">
						<span class="k">CPU</span>
						<span class="v accent">{system.cpu_pct.toFixed(1)} %</span>
					</span>
					<span class="readout">
						<span class="k">RAM</span>
						<span class="v">{(system.mem_used_mb / 1024).toFixed(1)} / {(system.mem_total_mb / 1024).toFixed(1)} GB</span>
					</span>
					<span class="readout">
						<span class="k">PROC</span>
						<span class="v">{system.proc_count}</span>
					</span>
				{:else}
					<span class="readout"><span class="k">—</span></span>
				{/if}
			</div>
		</header>
		{#if daemon.alive || ts.length > 0}
			<LiveChart {ts} {cpu} {mem} />
			<div class="legend label-tech">
				<span><i class="sw accent-bg"></i> cpu</span>
				<span><i class="sw dim-bg"></i> ram</span>
			</div>
		{:else}
			<p class="err">{error || 'služba neběží — graf čeká na data'}</p>
		{/if}
	</section>

	<!-- ── Tabulka procesů (v1: plochá, strom přijde s identitou ve v2) ── -->
	<section class="card table-card">
		<header class="card-head">
			<span class="label-tech">// processes</span>
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
						<tr>
							<td class="t-name">{p.name}</td>
							<td class="t-num value-mono">{p.pid}</td>
							<td class="t-num value-mono">{p.cpu_pct.toFixed(1)} %</td>
							<td class="t-num value-mono">{fmtMem(p.ws_bytes)}</td>
							<td class="t-num value-mono">{p.threads}</td>
						</tr>
					{:else}
						<tr>
							<td colspan="5" class="empty label-tech">
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
		align-items: baseline;
		justify-content: space-between;
		margin-bottom: 0.7rem;
	}

	.readouts {
		display: flex;
		gap: 1.4rem;
		font-size: 12px;
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

	.legend {
		display: flex;
		gap: 1.2rem;
		margin-top: 0.5rem;
	}
	.legend .sw {
		display: inline-block;
		width: 14px;
		height: 2px;
		vertical-align: middle;
		margin-right: 0.35rem;
	}
	.accent-bg {
		background: var(--accent);
		box-shadow: 0 0 6px color-mix(in srgb, var(--accent) 45%, transparent);
	}
	.dim-bg {
		background: var(--text-dim);
	}

	.err {
		margin: 0.6rem 0;
		color: var(--danger);
		font-size: 0.85rem;
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
		background: #1a1b21;
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
	}
	thead th:hover {
		color: var(--text-dim);
	}
	td {
		padding: 0.34rem 1rem;
		border-bottom: 1px solid var(--border);
		color: var(--text-dim);
		white-space: nowrap;
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
