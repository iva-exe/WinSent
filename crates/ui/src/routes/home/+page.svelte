<script>
	// Home (v4D, SPEC 9.2): grid dlaždic s živými daty ze všech sekcí.
	// Klik na dlaždici = skok do příslušné sekce.
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { invoke } from '@tauri-apps/api/core';
	import Num from '$lib/Num.svelte';
	import {
		Cpu,
		MemoryStick,
		Zap,
		HardDrive,
		Wifi,
		TriangleAlert,
		Blocks,
		Activity
	} from 'lucide-svelte';

	let system = $state(null);
	let procs = $state([]);
	let incidents = $state([]);
	let volumes = $state([]);
	let health = $state([]);
	let appCount = $state(null);

	function colorForLoad(v) {
		if (v == null) return 'var(--text-dim)';
		if (v <= 55) return 'var(--ok)';
		if (v <= 90) return 'var(--warn)';
		return 'var(--danger)';
	}
	function fmtBps(v) {
		if (v == null) return '—';
		const mb = v / (1024 * 1024);
		return mb >= 1 ? `${mb.toFixed(1)} MB/s` : `${(v / 1024).toFixed(0)} kB/s`;
	}
	function fmtSize(b) {
		if (b == null) return '—';
		return b >= 1e9 ? (b / 1e9).toFixed(0) + ' GB' : (b / 1e6).toFixed(0) + ' MB';
	}
	function fmtAgo(ts) {
		const s = Math.max(0, Math.floor(Date.now() / 1000 - ts));
		if (s < 3600) return `před ${Math.floor(s / 60)} min`;
		if (s < 86400) return `před ${Math.floor(s / 3600)} h`;
		return `před ${Math.floor(s / 86400)} dny`;
	}

	let topProcs = $derived.by(() =>
		[...procs].sort((a, b) => b.cpu_pct - a.cpu_pct).slice(0, 5)
	);
	let memPct = $derived(
		system?.mem_total_mb ? (system.mem_used_mb / system.mem_total_mb) * 100 : null
	);
	let lastIncident = $derived(incidents[0] ?? null);
	const kindLabel = {
		stall: 'zásek systému',
		app_crash: 'pád aplikace',
		bsod: 'BSOD / tvrdý pád'
	};

	async function pollFast() {
		try {
			system = await invoke('query_system');
			procs = await invoke('query_procs');
		} catch {
			system = null;
		}
	}
	async function pollSlow() {
		try {
			incidents = await invoke('query_incidents', { limit: 5 });
		} catch {
			incidents = [];
		}
		try {
			const v = await invoke('query_volumes');
			volumes = v.volumes;
			health = v.health;
		} catch {
			volumes = [];
		}
		try {
			appCount = (await invoke('query_apps')).length;
		} catch {
			appCount = null;
		}
	}

	onMount(() => {
		pollFast();
		pollSlow();
		const t1 = setInterval(pollFast, 1000);
		const t2 = setInterval(pollSlow, 20000);
		return () => {
			clearInterval(t1);
			clearInterval(t2);
		};
	});
</script>

<div class="page">
	<header class="head">
		<h1>Home</h1>
		<span class="sub">souhrn systému — klik na dlaždici otevře sekci</span>
	</header>

	<div class="grid">
		<!-- CPU / RAM / GPU / síť -->
		<button class="tile" onclick={() => goto('/tasks')}>
			<span class="t-head"><Cpu size={15} /> CPU</span>
			<span class="t-big mono" style:color={colorForLoad(system?.cpu_pct)}>
				{#if system}<Num value={system.cpu_pct} format={(v) => v.toFixed(0) + ' %'} />{:else}—{/if}
			</span>
			<span class="t-sub">{system?.cpu_clock_mhz ? (system.cpu_clock_mhz / 1000).toFixed(2) + ' GHz' : ''}</span>
		</button>
		<button class="tile" onclick={() => goto('/tasks')}>
			<span class="t-head"><MemoryStick size={15} /> RAM</span>
			<span class="t-big mono" style:color={colorForLoad(memPct)}>
				{#if memPct != null}<Num value={memPct} format={(v) => v.toFixed(0) + ' %'} />{:else}—{/if}
			</span>
			<span class="t-sub"
				>{system ? `${(system.mem_used_mb / 1024).toFixed(1)} / ${(system.mem_total_mb / 1024).toFixed(0)} GB` : ''}</span
			>
		</button>
		<button class="tile" onclick={() => goto('/tasks')}>
			<span class="t-head"><Zap size={15} /> GPU</span>
			<span class="t-big mono" style:color={colorForLoad(system?.gpu_pct)}>
				{#if system?.gpu_pct != null}<Num value={system.gpu_pct} format={(v) => v.toFixed(0) + ' %'} />{:else}—{/if}
			</span>
			<span class="t-sub">{system?.gpu?.temp_c != null ? system.gpu.temp_c + ' °C' : ''}</span>
		</button>
		<button class="tile" onclick={() => goto('/tasks')}>
			<span class="t-head"><Wifi size={15} /> Síť</span>
			<span class="t-mid mono net-down">↓ {fmtBps(system?.net_rx_bps)}</span>
			<span class="t-mid mono net-up">↑ {fmtBps(system?.net_tx_bps)}</span>
		</button>

		<!-- Top procesy -->
		<button class="tile wide tall" onclick={() => goto('/tasks')}>
			<span class="t-head"><Activity size={15} /> Top procesy</span>
			<ul class="t-list">
				{#each topProcs as p (p.pid)}
					<li>
						<span class="t-name">{p.app_name || p.name}</span>
						<span class="mono" style:color={colorForLoad(p.cpu_pct)}>{p.cpu_pct.toFixed(1)} %</span>
					</li>
				{/each}
			</ul>
		</button>

		<!-- Poslední incident -->
		<button class="tile wide" onclick={() => goto('/incidents')}>
			<span class="t-head warn-h"><TriangleAlert size={15} /> Poslední incident</span>
			{#if lastIncident}
				<span class="t-mid">{kindLabel[lastIncident.kind] ?? lastIncident.kind}
					{#if lastIncident.culprit}— {lastIncident.culprit}{/if}</span>
				<span class="t-sub">{fmtAgo(lastIncident.ts)}</span>
			{:else}
				<span class="t-mid dim">žádný — systém jede čistě</span>
			{/if}
		</button>

		<!-- Disky -->
		<button class="tile wide tall" onclick={() => goto('/files')}>
			<span class="t-head"><HardDrive size={15} /> Disky</span>
			<ul class="t-list">
				{#each volumes as v (v.letter)}
					{@const pct = v.total_bytes ? ((v.total_bytes - v.free_bytes) / v.total_bytes) * 100 : 0}
					<li>
						<span class="t-name mono">{v.letter}:</span>
						<span class="v-bar"><span
								class="v-fill"
								style:width="{pct}%"
								style:background={pct >= 90 ? 'var(--danger)' : pct >= 75 ? 'var(--warn)' : 'var(--ok)'}
							></span></span>
						<span class="mono dim">{fmtSize(v.free_bytes)} volných</span>
					</li>
				{/each}
				{#each health.filter((h) => h.temp_c != null) as h (h.index)}
					<li>
						<span class="t-name">{h.model}</span>
						<span class="mono" style:color={(h.used_pct ?? 0) >= 80 ? 'var(--warn)' : 'var(--ok)'}
							>{100 - Math.min(h.used_pct ?? 0, 100)} % život.</span
						>
					</li>
				{/each}
			</ul>
		</button>

		<!-- Aplikace -->
		<button class="tile" onclick={() => goto('/programs')}>
			<span class="t-head"><Blocks size={15} /> Aplikace</span>
			<span class="t-big mono">{appCount ?? '—'}</span>
			<span class="t-sub">{system ? system.proc_count + ' procesů běží' : ''}</span>
		</button>
	</div>
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
	.grid {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		grid-auto-rows: minmax(96px, auto);
		gap: 10px;
		overflow-y: auto;
	}
	.tile {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 6px;
		padding: 12px 14px;
		background: var(--surface);
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		color: var(--text);
		font: inherit;
		cursor: pointer;
		text-align: left;
	}
	.tile:hover {
		background: var(--surface-hover);
		border-color: var(--border-strong);
	}
	.tile.wide {
		grid-column: span 2;
	}
	.tile.tall {
		grid-row: span 2;
	}
	.t-head {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 0.72rem;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--text-dim);
	}
	.warn-h {
		color: var(--warn);
	}
	.t-big {
		font-size: 1.7rem;
		font-weight: 500;
	}
	.t-mid {
		font-size: 0.95rem;
	}
	.t-sub {
		font-size: 0.72rem;
		color: var(--text-faint);
	}
	.t-list {
		list-style: none;
		margin: 0;
		padding: 0;
		width: 100%;
		display: flex;
		flex-direction: column;
		gap: 5px;
	}
	.t-list li {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 0.8rem;
	}
	.t-name {
		flex: 1;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.v-bar {
		flex: 2;
		height: 5px;
		border-radius: 3px;
		background: var(--surface-hover);
		overflow: hidden;
	}
	.v-fill {
		display: block;
		height: 100%;
	}
	.net-down {
		color: var(--net-down);
	}
	.net-up {
		color: var(--net-up);
	}
	.mono {
		font-family: var(--font-mono);
	}
	.dim {
		color: var(--text-faint);
	}
</style>
