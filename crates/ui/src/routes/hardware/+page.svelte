<script>
	// Hardware (v9, SPEC kap. 15.4) — komponentově orientované karty.
	//
	// Klíčový princip: všechno o jedné komponentě je pohromadě u ní.
	// Karta má nahoře živý graf, pod ním údaje té komponenty. Ne
	// rozházené mezi obrazovkami.
	//
	// Druhý princip: nikdy nepředstírat číslo, které nemáme. U teploty
	// se vždy ukazuje zdroj; když ji nikdo nehlásí, řekne se to nahlas
	// a nabídne se odpověď na otázku, kvůli které se lidi ptají —
	// „brzdí mě něco?" (takty + throttling).
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import LiveChart from '$lib/LiveChart.svelte';
	import Num from '$lib/Num.svelte';
	import {
		Cpu,
		MemoryStick,
		HardDrive,
		Thermometer,
		BatteryCharging,
		CircuitBoard,
		Monitor,
		TriangleAlert
	} from 'lucide-svelte';

	let statics = $state(null);
	let hw = $state(null);
	let sys = $state(null);
	let loadError = $state('');

	// Historie pro grafy v kartách — vlastní kruhový buffer z 1Hz
	// vzorků, stejně jako Tasks.
	const KEEP = 900;
	let ts = $state([]);
	let cpuSeries = $state([]);
	let ramSeries = $state([]);
	let gpuSeries = $state([]);

	async function loadStatic() {
		try {
			statics = await invoke('query_sys_info');
		} catch (e) {
			loadError = String(e);
		}
	}

	async function loadHw() {
		try {
			hw = await invoke('query_hardware');
			loadError = '';
		} catch (e) {
			loadError = String(e);
		}
	}

	async function tick() {
		try {
			const s = await invoke('query_system');
			sys = s;
			const now = Math.floor(Date.now() / 1000);
			ts = [...ts, now].slice(-KEEP);
			cpuSeries = [...cpuSeries, s.cpu_pct].slice(-KEEP);
			ramSeries = [
				...ramSeries,
				s.mem_total_mb ? (s.mem_used_mb / s.mem_total_mb) * 100 : 0
			].slice(-KEEP);
			gpuSeries = [...gpuSeries, s.gpu_pct ?? 0].slice(-KEEP);
		} catch {
			/* služba mimo — graf zůstane stát */
		}
	}

	onMount(() => {
		loadStatic();
		loadHw();
		tick();
		const t1 = setInterval(tick, 1000);
		// Tepelná kaskáda sahá na WMI — po sekundách, ne v cyklu
		// (SPEC 15.2). Služba to navíc ještě cachuje.
		const t2 = setInterval(loadHw, 8000);
		return () => {
			clearInterval(t1);
			clearInterval(t2);
		};
	});

	function fmtGb(mb) {
		return (mb / 1024).toFixed(1) + ' GB';
	}

	function fmtBytes(b) {
		if (b == null) return '—';
		if (b >= 1e12) return (b / 1e12).toFixed(2) + ' TB';
		if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB';
		return (b / 1e6).toFixed(0) + ' MB';
	}

	function fmtHours(h) {
		if (h == null) return '—';
		if (h >= 8760) return (h / 8760).toFixed(1) + ' roku provozu';
		if (h >= 24) return Math.round(h / 24) + ' dní provozu';
		return h + ' h provozu';
	}

	function fmtRemaining(s) {
		if (s == null) return null;
		const h = Math.floor(s / 3600);
		const m = Math.floor((s % 3600) / 60);
		return h > 0 ? `${h} h ${m} min` : `${m} min`;
	}

	// Svazky patřící k danému fyzickému disku.
	function volumesOf(index) {
		return (hw?.volumes ?? []).filter((v) => v.disk_index === index);
	}

	// Teplota nemá univerzální stupnici — 80 °C je u disku alarm,
	// u procesoru běžná zátěž. Proto prahy podle komponenty.
	function tempClass(c, warn, hot) {
		if (c == null) return '';
		return c >= hot ? 'hot' : c >= warn ? 'warm' : 'cool';
	}
</script>

<div class="page">
	{#if loadError}
		<p class="empty">Nelze načíst hardware: {loadError}</p>
	{/if}

	<!-- ── CPU ── -->
	<section class="card">
		<header>
			<Cpu size={15} />
			<h2>{statics?.cpu_name ?? 'Procesor'}</h2>
			{#if sys}
				<span class="live"><Num value={sys.cpu_pct} digits={0} /> %</span>
			{/if}
		</header>
		<div class="chart">
			<LiveChart {ts} values={cpuSeries} mode="cpu" />
		</div>
		<dl>
			<div>
				<dt>Jádra</dt>
				<dd>{statics?.physical_cores ?? '—'} fyzická / {statics?.logical_cores ?? '—'} logická</dd>
			</div>
			<div>
				<dt>Takt</dt>
				<dd>
					{hw?.cpu_thermal?.clock_mhz ?? '—'} MHz
					<span class="dim">/ max {hw?.cpu_thermal?.max_mhz ?? '—'} MHz</span>
				</dd>
			</div>
			<div>
				<dt><Thermometer size={12} /> Teplota</dt>
				<dd>
					{#if hw?.cpu_thermal?.celsius != null}
						<span class={tempClass(hw.cpu_thermal.celsius, 75, 90)}>
							{hw.cpu_thermal.celsius.toFixed(0)} °C
						</span>
						<span class="src">zdroj: {hw.cpu_thermal.temp_source}</span>
					{:else}
						<span class="dim">nedostupná</span>
						<span class="src">
							tenhle stroj teplotu procesoru z Windows nehlásí — ukázala by se, kdyby běžel
							HWiNFO nebo LibreHardwareMonitor
						</span>
					{/if}
				</dd>
			</div>
			<div>
				<dt>Brzdí ho něco?</dt>
				<dd>
					{#if hw?.cpu_thermal?.throttling}
						<span class="warn"><TriangleAlert size={12} /> běží pod svým maximem</span>
					{:else}
						<span class="ok">ne, jede naplno</span>
					{/if}
				</dd>
			</div>
			<div>
				<dt>Cache</dt>
				<dd class="dim">
					L1 {statics?.l1_kb ?? '—'} kB · L2 {statics?.l2_kb ?? '—'} kB · L3 {statics?.l3_kb ??
						'—'} kB
				</dd>
			</div>
		</dl>
	</section>

	<!-- ── Paměť ── -->
	<section class="card">
		<header>
			<MemoryStick size={15} />
			<h2>Paměť</h2>
			{#if sys}
				<span class="live">{fmtGb(sys.mem_used_mb)} / {fmtGb(sys.mem_total_mb)}</span>
			{/if}
		</header>
		<div class="chart">
			<LiveChart {ts} values={ramSeries} mode="ram" />
		</div>
		<dl>
			<div>
				<dt>Osazeno</dt>
				<dd>{statics?.ram_modules?.length ?? 0} modulů ve {statics?.ram_slots ?? '—'} slotech</dd>
			</div>
		</dl>
		{#if statics?.ram_modules?.length}
			<table>
				<thead>
					<tr>
						<th>Slot</th><th>Velikost</th><th>Běží na</th><th>Modul umí</th><th>Výrobce</th>
					</tr>
				</thead>
				<tbody>
					{#each statics.ram_modules as m (m.slot + m.part_number)}
						<tr>
							<td class="mono">{m.slot}</td>
							<td>{(m.size_mb / 1024).toFixed(0)} GB</td>
							<td>{m.configured_mts || '—'} MT/s</td>
							<td class="dim">{m.speed_mts || '—'} MT/s</td>
							<td class="dim">{m.manufacturer} {m.part_number}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</section>

	<!-- ── GPU: karta jen když ji umíme číst; jinak nic nepředstíráme ── -->
	{#if statics?.gpu_name}
		<section class="card">
			<header>
				<Monitor size={15} />
				<h2>{statics.gpu_name}</h2>
				{#if sys?.gpu_pct != null}
					<span class="live"><Num value={sys.gpu_pct} digits={0} /> %</span>
				{/if}
			</header>
			{#if sys?.gpu_pct != null}
				<div class="chart">
					<LiveChart {ts} values={gpuSeries} mode="gpu" />
				</div>
			{/if}
			<dl>
				{#if sys?.gpu?.temp_c != null}
					<div>
						<dt><Thermometer size={12} /> Teplota</dt>
						<dd><span class={tempClass(sys.gpu.temp_c, 75, 88)}>{sys.gpu.temp_c} °C</span></dd>
					</div>
				{/if}
				{#if sys?.gpu?.vram_used_mb != null}
					<div>
						<dt>VRAM</dt>
						<dd>{fmtGb(sys.gpu.vram_used_mb)} / {fmtGb(sys.gpu.vram_total_mb ?? 0)}</dd>
					</div>
				{/if}
				{#if sys?.gpu?.power_w != null}
					<div><dt>Spotřeba</dt><dd>{sys.gpu.power_w.toFixed(0)} W</dd></div>
				{/if}
				{#if sys?.gpu_pct == null}
					<div>
						<dt>Telemetrie</dt>
						<dd class="dim">
							tahle karta podrobnosti přes ovladač nehlásí — vidíme jen její jméno
						</dd>
					</div>
				{/if}
			</dl>
		</section>
	{/if}

	<!-- ── Disky: každý fyzický disk vlastní karta i se svazky ── -->
	{#each hw?.disks ?? [] as d (d.index)}
		<section class="card">
			<header>
				<HardDrive size={15} />
				<h2>{d.model || `Disk ${d.index}`}</h2>
				{#if d.critical}
					<span class="live warn"><TriangleAlert size={12} /> SMART hlásí problém</span>
				{/if}
			</header>
			{#each volumesOf(d.index) as v (v.letter)}
				{@const pct = v.total_bytes ? ((v.total_bytes - v.free_bytes) / v.total_bytes) * 100 : 0}
				<div class="vol">
					<div class="vol-head">
						<span class="mono">{v.letter}:</span>
						<span class="dim">{v.label || v.fs}</span>
						<span class="vol-num"
							>{fmtBytes(v.free_bytes)} volných z {fmtBytes(v.total_bytes)}</span
						>
					</div>
					<div class="bar" class:full={pct >= 90}>
						<div class="fill" style="width: {pct}%"></div>
					</div>
				</div>
			{/each}
			<dl>
				<div>
					<dt><Thermometer size={12} /> Teplota</dt>
					<dd>
						{#if d.temp_c != null}
							<span class={tempClass(d.temp_c, 55, 70)}>{d.temp_c} °C</span>
						{:else}
							<span class="dim">nehlásí</span>
							<span class="src">zdraví přes SMART umí NVMe disky; tenhle ho nedává</span>
						{/if}
					</dd>
				</div>
				{#if d.used_pct != null}
					<div>
						<dt>Opotřebení</dt>
						<dd>
							<span class={d.used_pct >= 80 ? 'warn' : 'ok'}>{d.used_pct} %</span>
							<span class="src">návrhové životnosti</span>
						</dd>
					</div>
				{/if}
				{#if d.spare_pct != null}
					<div><dt>Rezervní bloky</dt><dd>{d.spare_pct} %</dd></div>
				{/if}
				{#if d.power_on_hours != null}
					<div><dt>Naběháno</dt><dd>{fmtHours(d.power_on_hours)}</dd></div>
				{/if}
			</dl>
		</section>
	{/each}

	<!-- ── Baterie: jen u strojů, které ji mají ── -->
	{#if hw?.battery}
		<section class="card">
			<header>
				<BatteryCharging size={15} />
				<h2>Baterie</h2>
				{#if hw.battery.percent != null}
					<span class="live">{hw.battery.percent} %</span>
				{/if}
			</header>
			<dl>
				<div>
					<dt>Stav</dt>
					<dd>
						{#if hw.battery.charging}
							nabíjí se
						{:else if hw.battery.ac_online}
							v síti, nenabíjí
						{:else}
							na baterii
							{#if fmtRemaining(hw.battery.remaining_s)}
								<span class="dim"> — zbývá asi {fmtRemaining(hw.battery.remaining_s)}</span>
							{/if}
						{/if}
					</dd>
				</div>
				<div>
					<dt>Opotřebení</dt>
					<dd>
						{#if hw.battery.wear_pct != null}
							<span class={hw.battery.wear_pct >= 30 ? 'warn' : 'ok'}
								>{hw.battery.wear_pct.toFixed(0)} %</span
							>
							<span class="src">
								dnes se nabije na {(hw.battery.full_mwh / 1000).toFixed(1)} Wh z původních
								{(hw.battery.design_mwh / 1000).toFixed(1)} Wh
							</span>
						{:else}
							<span class="dim">baterie kapacity nehlásí</span>
						{/if}
					</dd>
				</div>
				{#if hw.battery.cycles != null}
					<div><dt>Nabíjecích cyklů</dt><dd>{hw.battery.cycles}</dd></div>
				{/if}
			</dl>
		</section>
	{/if}

	<!-- ── Deska a firmware ── -->
	{#if hw?.board}
		<section class="card">
			<header>
				<CircuitBoard size={15} />
				<h2>Základní deska</h2>
			</header>
			<dl>
				<div>
					<dt>Deska</dt>
					<dd>
						{hw.board.manufacturer}
						{hw.board.product}
						{#if hw.board.version}<span class="dim">({hw.board.version})</span>{/if}
					</dd>
				</div>
				{#if hw.board.system_product}
					<div>
						<dt>Stroj</dt>
						<dd class="dim">{hw.board.system_manufacturer} {hw.board.system_product}</dd>
					</div>
				{/if}
				<div>
					<dt>BIOS / UEFI</dt>
					<dd>
						{hw.board.bios_vendor}
						{hw.board.bios_version}
						<span class="dim">z {hw.board.bios_date}</span>
					</dd>
				</div>
			</dl>
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
		padding-right: 4px;
	}
	.card {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 12px 14px 14px;
	}
	.card header {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-bottom: 10px;
		color: var(--text-dim);
	}
	.card h2 {
		font-size: 0.9rem;
		font-weight: 600;
		color: var(--text);
		margin: 0;
	}
	.live {
		margin-left: auto;
		font-variant-numeric: tabular-nums;
		font-size: 0.85rem;
		color: var(--text);
	}
	.chart {
		height: 150px;
		margin-bottom: 12px;
	}
	dl {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
		gap: 8px 18px;
		margin: 0;
	}
	dl > div {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
	}
	dt {
		display: flex;
		align-items: center;
		gap: 4px;
		font-size: 0.72rem;
		color: var(--text-dim);
		text-transform: uppercase;
		letter-spacing: 0.03em;
	}
	dd {
		margin: 0;
		font-size: 0.85rem;
		font-variant-numeric: tabular-nums;
	}
	/* Zdroj údaje patří k číslu — uživatel má vždy vědět, čemu věří. */
	.src {
		display: block;
		font-size: 0.72rem;
		color: var(--text-dim);
		font-variant-numeric: normal;
	}
	.dim {
		color: var(--text-dim);
	}
	.mono {
		font-family: var(--mono);
	}
	.ok {
		color: var(--ok);
	}
	.warn {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--warn);
	}
	.cool {
		color: var(--ok);
	}
	.warm {
		color: var(--warn);
	}
	.hot {
		color: var(--danger);
	}

	table {
		width: 100%;
		border-collapse: collapse;
		margin-top: 10px;
		font-size: 0.8rem;
	}
	th {
		text-align: left;
		font-weight: 500;
		font-size: 0.72rem;
		text-transform: uppercase;
		letter-spacing: 0.03em;
		color: var(--text-dim);
		padding: 4px 8px 4px 0;
		border-bottom: 1px solid var(--border);
	}
	td {
		padding: 5px 8px 5px 0;
		border-bottom: 1px solid var(--border);
		font-variant-numeric: tabular-nums;
	}

	.vol {
		margin-bottom: 10px;
	}
	.vol-head {
		display: flex;
		align-items: baseline;
		gap: 8px;
		font-size: 0.8rem;
		margin-bottom: 4px;
	}
	.vol-num {
		margin-left: auto;
		color: var(--text-dim);
		font-variant-numeric: tabular-nums;
	}
	.bar {
		height: 6px;
		border-radius: 3px;
		background: var(--border);
		overflow: hidden;
	}
	.fill {
		height: 100%;
		background: var(--accent);
	}
	.bar.full .fill {
		background: var(--danger);
	}
	.empty {
		color: var(--text-dim);
		font-size: 0.85rem;
	}
</style>
