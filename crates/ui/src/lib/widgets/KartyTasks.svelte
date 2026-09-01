<script>
	// Dlaždice ze sekce Tasks — všechno, co se mění každou sekundu.
	//
	// Jedna komponenta na celou skupinu: widgety sdílejí tytéž pomocné
	// funkce i tentýž vzorek dat, takže deset samostatných souborů by
	// znamenalo jen desetkrát opsat záhlaví.
	import { goto } from '$app/navigation';
	import Num from '$lib/Num.svelte';
	import MiniGraf from './MiniGraf.svelte';
	import AppIcon from '$lib/AppIcon.svelte';
	import { data, serie } from './data.svelte.js';
	import { ikony, chciIkonu } from './ikony.svelte.js';
	import { barvaZateze, bps, velikost, doba } from './pomoc.js';

	let { typ, velikost: rozmer } = $props();

	let siroka = $derived(rozmer !== 'mala');
	let s = $derived(data.system);

	let memPct = $derived(s?.mem_total_mb ? (s.mem_used_mb / s.mem_total_mb) * 100 : null);

	// ── graf ─────────────────────────────────────────────────────────
	let metrika = $state('cpu');
	const METRIKY = [
		{ id: 'cpu', label: 'CPU' },
		{ id: 'ram', label: 'RAM' },
		{ id: 'gpu', label: 'GPU' },
		{ id: 'net', label: 'Síť' }
	];
	let ramPct = $derived.by(() => {
		const total = s?.mem_total_mb;
		return total ? serie.ramMb.map((mb) => (mb / total) * 100) : [];
	});
	let grafData = $derived.by(() => {
		if (metrika === 'cpu') return { v: serie.cpu, v2: null, skala: 'pct', barva: 'var(--ok)' };
		if (metrika === 'ram') return { v: ramPct, v2: null, skala: 'pct', barva: 'var(--net-up)' };
		if (metrika === 'gpu') return { v: serie.gpu, v2: null, skala: 'pct', barva: 'var(--warn)' };
		return { v: serie.rx, v2: serie.tx, skala: 'auto', barva: 'var(--net-down)' };
	});
	let grafTeď = $derived.by(() => {
		if (metrika === 'cpu') return s ? `${s.cpu_pct.toFixed(0)} %` : '—';
		if (metrika === 'ram') return memPct != null ? `${memPct.toFixed(0)} %` : '—';
		if (metrika === 'gpu') return s?.gpu_pct != null ? `${s.gpu_pct.toFixed(0)} %` : 'nehlásí';
		return `${bps(s?.net_rx_bps)} ↓`;
	});

	// ── žrouti ───────────────────────────────────────────────────────
	let podle = $state('cpu');
	const ZROUTI = [
		{ id: 'cpu', label: 'CPU' },
		{ id: 'ram', label: 'Paměť' },
		{ id: 'disk', label: 'Disk' },
		{ id: 'gpu', label: 'GPU' }
	];
	let zrouti = $derived.by(() => {
		const rows = data.procs ?? [];
		const m = new Map();
		for (const p of rows) {
			const key = p.identity_key || `pid:${p.pid}`;
			const a = m.get(key) ?? {
				key,
				name: p.app_name || p.name,
				cpu: 0,
				ram: 0,
				disk: 0,
				gpu: 0
			};
			a.cpu += p.cpu_pct;
			a.ram += p.ws_bytes;
			a.disk += p.disk_r_bps + p.disk_w_bps;
			a.gpu += p.gpu_pct;
			m.set(key, a);
		}
		const out = [...m.values()].sort((a, b) => b[podle] - a[podle]);
		return out.slice(0, siroka ? 6 : 3).filter((a) => a[podle] > 0);
	});
	// Ikony se dotahují na pozadí; cache je společná všem dlaždicím,
	// takže tentýž klíč se přes pipe žádá jednou, ne v každé z nich.
	$effect(() => {
		for (const a of zrouti) chciIkonu(a.key);
	});

	function hodnotaZrouta(a) {
		if (podle === 'cpu') return `${a.cpu.toFixed(1)} %`;
		if (podle === 'ram') return velikost(a.ram);
		if (podle === 'disk') return bps(a.disk);
		return `${a.gpu.toFixed(0)} %`;
	}

	// ── disky ────────────────────────────────────────────────────────
	let diskyRady = $derived.by(() => {
		const jmena = new Map((data.sysInfo?.disks ?? []).map((d) => [d.index, d.model]));
		return (s?.disks ?? []).map((d) => ({
			...d,
			model: jmena.get(d.index) ?? `disk ${d.index}`
		}));
	});

	// ── proč to seká ─────────────────────────────────────────────────
	// Ostatní signály se sčítají do jedné věty. Tři nuly vedle sebe
	// vypadají jako rozbité čtení; jedna věta „vše ostatní v normě"
	// říká totéž a nechá prostor tomu jedinému číslu, které se hýbe.
	let vNorme = $derived(
		s ? !s.thermal_throttle && s.disk_qlen < 2 && s.disk_lat_ms < 20 : true
	);
	let potize = $derived.by(() => {
		if (!s) return [];
		const out = [];
		if (s.thermal_throttle) out.push('procesor se přehřívá a zpomaluje');
		if (s.disk_qlen >= 2) out.push(`fronta disku ${s.disk_qlen.toFixed(1)}`);
		if (s.disk_lat_ms >= 20) out.push(`latence disku ${s.disk_lat_ms.toFixed(0)} ms`);
		return out;
	});
</script>

{#if typ === 'cpu'}
	<span class="w-big" style:color={barvaZateze(s?.cpu_pct)}>
		{#if s}<Num value={s.cpu_pct} format={(v) => v.toFixed(0) + ' %'} />{:else}—{/if}
	</span>
	<span class="w-sub">
		{s?.cpu_clock_mhz ? `${(s.cpu_clock_mhz / 1000).toFixed(2)} GHz` : ''}
		{#if s?.cpu_clock_max_mhz}<span class="w-dim"> / {(s.cpu_clock_max_mhz / 1000).toFixed(1)}</span>{/if}
	</span>
	{#if siroka}<MiniGraf values={serie.cpu} vyska={34} barva="var(--ok)" />{/if}
{:else if typ === 'ram'}
	<span class="w-big" style:color={barvaZateze(memPct)}>
		{#if memPct != null}<Num value={memPct} format={(v) => v.toFixed(0) + ' %'} />{:else}—{/if}
	</span>
	<span class="w-sub">
		{s ? `${(s.mem_used_mb / 1024).toFixed(1)} z ${(s.mem_total_mb / 1024).toFixed(0)} GB` : ''}
	</span>
	{#if siroka}<MiniGraf values={ramPct} vyska={34} barva="var(--net-up)" />{/if}
{:else if typ === 'gpu'}
	{#if s?.gpu_pct == null}
		<span class="w-empty">Grafika využití nehlásí — bez NVML se nedá odhadovat.</span>
	{:else}
		<span class="w-big" style:color={barvaZateze(s.gpu_pct)}>
			<Num value={s.gpu_pct} format={(v) => v.toFixed(0) + ' %'} />
		</span>
		<span class="w-sub">
			{#if s.gpu?.vram_used_mb != null && s.gpu?.vram_total_mb}
				{(s.gpu.vram_used_mb / 1024).toFixed(1)} / {(s.gpu.vram_total_mb / 1024).toFixed(0)} GB VRAM
			{/if}
			{#if s.gpu?.temp_c != null}· {s.gpu.temp_c} °C{/if}
		</span>
		{#if siroka}<MiniGraf values={serie.gpu} vyska={34} barva="var(--warn)" />{/if}
	{/if}
{:else if typ === 'graf'}
	<div class="w-segs">
		{#each METRIKY as m (m.id)}
			<button class="w-seg" class:on={metrika === m.id} onclick={() => (metrika = m.id)}>
				{m.label}
			</button>
		{/each}
		<span class="ted w-mono">{grafTeď}</span>
	</div>
	<div class="rost">
		<MiniGraf
			values={grafData.v}
			values2={grafData.v2}
			skala={grafData.skala}
			barva={grafData.barva}
			vyska={rozmer === 'stredni' ? 44 : rozmer === 'velka' ? 110 : 118}
		/>
	</div>
	<span class="w-sub">
		{#if serie.ts.length < 60}
			posledních {serie.ts.length} s
		{:else}
			{@const m = Math.round(serie.ts.length / 60)}
			{m === 1 ? 'poslední minuta' : m <= 4 ? `poslední ${m} minuty` : `posledních ${m} minut`}
		{/if}
		{#if metrika === 'net'}· ↑ {bps(s?.net_tx_bps)}{/if}
	</span>
{:else if typ === 'jadra'}
	{#if s?.cores?.length}
		<div class="jadra">
			{#each s.cores as c, i (i)}
				<div class="jadro" title="jádro {i}: {c.toFixed(0)} %">
					<div class="jbar" style:height="{Math.max(2, c)}%" style:background={barvaZateze(c)}></div>
				</div>
			{/each}
		</div>
		<span class="w-sub">{s.cores.length} logických jader · max {Math.max(...s.cores).toFixed(0)} %</span>
	{:else}
		<span class="w-empty">Zátěž jader se ještě nenačetla.</span>
	{/if}
{:else if typ === 'zrouti'}
	<div class="w-segs">
		{#each ZROUTI as z (z.id)}
			<button class="w-seg" class:on={podle === z.id} onclick={() => (podle = z.id)}>{z.label}</button>
		{/each}
	</div>
	<ul class="w-list scroll">
		{#each zrouti as a (a.key)}
			<li>
				<button class="w-klik w-row" onclick={() => goto('/tasks?q=' + encodeURIComponent(a.name))}>
					<AppIcon src={ikony[a.key]} name={a.name} size={15} />
					<span class="w-name">{a.name}</span>
					<span class="w-mono" style:color={podle === 'cpu' ? barvaZateze(a.cpu) : 'var(--text-dim)'}>
						{hodnotaZrouta(a)}
					</span>
				</button>
			</li>
		{/each}
		{#if !zrouti.length}
			<li class="w-empty">Nic zrovna nezatěžuje.</li>
		{/if}
	</ul>
{:else if typ === 'seka'}
	<span class="w-big" style:color={s && s.hard_flt_rate > 50 ? 'var(--warn)' : 'var(--text)'}>
		{s ? s.hard_flt_rate.toFixed(0) : '—'}
	</span>
	<span class="w-sub">hard faultů za sekundu</span>
	{#if potize.length}
		<ul class="w-list">
			{#each potize as p (p)}
				<li class="w-row" style:color="var(--warn)">{p}</li>
			{/each}
		</ul>
	{:else if vNorme}
		<span class="w-sub">fronta disku, latence i teploty v normě</span>
	{/if}
{:else if typ === 'uptime'}
	<span class="w-big">{s ? doba(s.uptime_s) : '—'}</span>
	<span class="w-sub">
		{#if s}{s.proc_count} procesů · {s.threads_total} vláken{/if}
	</span>
	{#if siroka && s}
		<span class="w-sub">{s.handles_total.toLocaleString('cs-CZ')} handlů otevřeno</span>
	{/if}
{:else if typ === 'diskrychlost'}
	<ul class="w-list">
		{#each diskyRady as d (d.index)}
			<li class="w-row">
				<span class="w-name">{d.model}</span>
				<span class="w-mono" style:color="var(--net-down)">↓ {bps(d.r_bps)}</span>
				<span class="w-mono" style:color="var(--net-up)">↑ {bps(d.w_bps)}</span>
			</li>
		{/each}
		{#if !diskyRady.length}<li class="w-empty">Rychlosti disků se ještě nenačetly.</li>{/if}
	</ul>
{:else if typ === 'samotny'}
	{@const u = data.samotny}
	<span class="w-big" style:color={barvaZateze(u?.cpu_pct)}>
		{u ? `${u.cpu_pct.toFixed(1)} %` : '—'}
	</span>
	<span class="w-sub">
		{#if u}služba {velikost(u.ws_bytes)} · databáze {velikost(u.db_bytes)}{/if}
	</span>
{/if}

<style>
	.ted {
		margin-left: auto;
		font-size: var(--fs-xs);
		color: var(--text-dim);
		align-self: center;
	}
	.rost {
		flex: 1;
		min-height: 0;
		display: flex;
		align-items: flex-end;
	}
	.rost :global(canvas) {
		width: 100%;
	}
	.jadra {
		display: flex;
		align-items: flex-end;
		gap: 2px;
		height: 100%;
		min-height: 30px;
		margin-bottom: 6px;
	}
	.jadro {
		flex: 1;
		min-width: 3px;
		height: 100%;
		display: flex;
		align-items: flex-end;
		background: var(--surface-hover);
		border-radius: 2px;
		overflow: hidden;
	}
	.jbar {
		width: 100%;
		border-radius: 2px;
		transition: height 0.25s ease;
	}
</style>
