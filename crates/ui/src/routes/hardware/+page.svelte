<script>
	// Hardware (v9, SPEC kap. 15) — soupis všeho, co v počítači je.
	//
	// Cíl téhle obrazovky je úplnost a čitelnost, ne grafy. Historii
	// a interaktivní křivky má Tasks; tady jsou jen malé sparkliny
	// „jak si to vede právě teď" a pod nimi má každý údaj vlastní
	// pole s vlastním popiskem. Nic se nepřekrývá.
	//
	// Pravidlo ze SPEC 15.2: nikdy nepředstírat číslo, které nemáme.
	// U teploty se vždy ukazuje zdroj; když ji nikdo nehlásí, řekne
	// se to nahlas.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import Sparkline from '$lib/Sparkline.svelte';
	import {
		Cpu,
		MemoryStick,
		HardDrive,
		Thermometer,
		BatteryCharging,
		CircuitBoard,
		Monitor,
		Search,
		TriangleAlert,
		ChevronRight
	} from 'lucide-svelte';

	let statics = $state(null);
	let hw = $state(null);
	let sys = $state(null);
	// Obrazovky čte UI proces, ne služba — ta běží v session 0, kde
	// žádná plocha není a seznam by byl prázdný.
	let displays = $state([]);
	let loadError = $state('');

	// Malý kruhový buffer jen pro sparkliny — 60 s stačí.
	const KEEP = 60;
	let cpuSeries = $state([]);
	let ramSeries = $state([]);
	let gpuSeries = $state([]);

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
			cpuSeries = [...cpuSeries, s.cpu_pct].slice(-KEEP);
			ramSeries = [
				...ramSeries,
				s.mem_total_mb ? (s.mem_used_mb / s.mem_total_mb) * 100 : 0
			].slice(-KEEP);
			if (s.gpu_pct != null) gpuSeries = [...gpuSeries, s.gpu_pct].slice(-KEEP);
		} catch {
			/* služba mimo — hodnoty zůstanou stát */
		}
	}

	onMount(() => {
		invoke('query_sys_info')
			.then((s) => (statics = s))
			.catch((e) => (loadError = String(e)));
		invoke('query_displays')
			.then((d) => (displays = d))
			.catch(() => (displays = []));
		loadHw();
		tick();
		const t1 = setInterval(tick, 1000);
		// Tepelná kaskáda sahá na WMI a soupis zařízení na SetupAPI —
		// po sekundách, ne v cyklu (SPEC 15.2). Služba to ještě cachuje.
		const t2 = setInterval(loadHw, 8000);
		return () => {
			clearInterval(t1);
			clearInterval(t2);
		};
	});

	function gb(mb) {
		return mb == null ? '—' : (mb / 1024).toFixed(1) + ' GB';
	}

	function bytes(b) {
		if (b == null) return '—';
		if (b >= 1e12) return (b / 1e12).toFixed(2) + ' TB';
		if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB';
		return (b / 1e6).toFixed(0) + ' MB';
	}

	function hours(h) {
		if (h == null) return '—';
		if (h >= 8760) return (h / 8760).toFixed(1) + ' roku';
		if (h >= 24) return Math.round(h / 24) + ' dní';
		return h + ' h';
	}

	function remaining(s) {
		if (s == null) return null;
		const h = Math.floor(s / 3600);
		const m = Math.floor((s % 3600) / 60);
		return h > 0 ? `${h} h ${m} min` : `${m} min`;
	}

	// Teplota nemá univerzální stupnici — 55 °C je u disku hodně,
	// u procesoru nic. Proto prahy podle komponenty.
	function tempClass(c, warn, hot) {
		if (c == null) return '';
		return c >= hot ? 'hot' : c >= warn ? 'warm' : 'cool';
	}

	function volumesOf(index) {
		return (hw?.volumes ?? []).filter((v) => v.disk_index === index);
	}

	// ── Soupis zařízení: seskupený podle tříd, s hledáním ──
	let devFilter = $state('');
	let openClass = $state(new Set());

	function toggleClass(name) {
		const s = new Set(openClass);
		if (s.has(name)) s.delete(name);
		else s.add(name);
		openClass = s;
	}

	let deviceGroups = $derived.by(() => {
		const q = devFilter.trim().toLowerCase();
		const map = new Map();
		for (const d of hw?.devices ?? []) {
			if (
				q &&
				!d.name.toLowerCase().includes(q) &&
				!d.manufacturer.toLowerCase().includes(q) &&
				!(d.class_desc || d.class).toLowerCase().includes(q)
			) {
				continue;
			}
			const key = d.class_desc || d.class || 'Ostatní';
			if (!map.has(key)) map.set(key, []);
			map.get(key).push(d);
		}
		return [...map.entries()]
			.map(([name, items]) => ({
				name,
				items,
				problems: items.filter((d) => d.problem_code !== 0).length
			}))
			.sort((a, b) => a.name.localeCompare(b.name, 'cs'));
	});

	let problemCount = $derived((hw?.devices ?? []).filter((d) => d.problem_code !== 0).length);
</script>

<div class="page">
	{#if loadError}
		<p class="empty">Nelze načíst hardware: {loadError}</p>
	{/if}

	<!-- ── Živý stav: tři dlaždice, každá svůj prostor ── -->
	<div class="vitals">
		<div class="tile">
			<div class="tile-head"><Cpu size={14} /> Procesor</div>
			<div class="tile-val">{sys ? Math.round(sys.cpu_pct) : '—'} <small>%</small></div>
			<Sparkline values={cpuSeries} height={30} />
			<div class="tile-sub">{hw?.cpu_thermal?.clock_mhz ?? '—'} MHz</div>
		</div>
		<div class="tile">
			<div class="tile-head"><MemoryStick size={14} /> Paměť</div>
			<div class="tile-val">
				{sys ? Math.round((sys.mem_used_mb / sys.mem_total_mb) * 100) : '—'} <small>%</small>
			</div>
			<Sparkline values={ramSeries} height={30} />
			<div class="tile-sub">{gb(sys?.mem_used_mb)} z {gb(sys?.mem_total_mb)}</div>
		</div>
		<div class="tile">
			<div class="tile-head"><Monitor size={14} /> Grafika</div>
			{#if sys?.gpu_pct != null}
				<div class="tile-val">{Math.round(sys.gpu_pct)} <small>%</small></div>
				<Sparkline values={gpuSeries} height={30} />
				<div class="tile-sub">
					{sys?.gpu?.temp_c != null ? `${Math.round(sys.gpu.temp_c)} °C` : 'zatížení karty'}
				</div>
			{:else}
				<div class="tile-val dim">—</div>
				<div class="tile-none">karta zatížení přes ovladač nehlásí</div>
			{/if}
		</div>
	</div>

	<!-- ── Procesor ── -->
	<section class="card">
		<h2><Cpu size={14} /> {statics?.cpu_name ?? 'Procesor'}</h2>
		<dl>
			<div><dt>Fyzická jádra</dt><dd>{statics?.physical_cores ?? '—'}</dd></div>
			<div><dt>Logická jádra</dt><dd>{statics?.logical_cores ?? '—'}</dd></div>
			<div><dt>Aktuální takt</dt><dd>{hw?.cpu_thermal?.clock_mhz ?? '—'} MHz</dd></div>
			<div><dt>Maximální takt</dt><dd>{hw?.cpu_thermal?.max_mhz ?? '—'} MHz</dd></div>
			<div>
				<dt><Thermometer size={11} /> Teplota</dt>
				{#if hw?.cpu_thermal?.celsius != null}
					<dd>
						<span class={tempClass(hw.cpu_thermal.celsius, 75, 90)}>
							{Math.round(hw.cpu_thermal.celsius)} °C
						</span>
					</dd>
					<p class="note">zdroj: {hw.cpu_thermal.temp_source}</p>
				{:else}
					<dd class="dim">nedostupná</dd>
					<p class="note">
						tenhle stroj ji z Windows nehlásí — objeví se, když poběží HWiNFO nebo
						LibreHardwareMonitor
					</p>
				{/if}
			</div>
			<div>
				<dt>Brzdí ho něco?</dt>
				{#if hw?.cpu_thermal?.throttling}
					<dd class="warn"><TriangleAlert size={12} /> běží pod maximem</dd>
				{:else}
					<dd class="ok">ne, jede naplno</dd>
				{/if}
			</div>
			<div><dt>Cache L1</dt><dd>{statics?.l1_kb ?? '—'} kB</dd></div>
			<div><dt>Cache L2</dt><dd>{statics?.l2_kb ?? '—'} kB</dd></div>
			<div><dt>Cache L3</dt><dd>{statics?.l3_kb ?? '—'} kB</dd></div>
		</dl>
	</section>

	<!-- ── Paměť ── -->
	<section class="card">
		<h2><MemoryStick size={14} /> Paměť</h2>
		<dl>
			<div><dt>Celkem</dt><dd>{gb(sys?.mem_total_mb)}</dd></div>
			<div><dt>Osazeno modulů</dt><dd>{statics?.ram_modules?.length ?? 0}</dd></div>
			<div><dt>Slotů na desce</dt><dd>{statics?.ram_slots ?? '—'}</dd></div>
		</dl>
		{#if statics?.ram_modules?.length}
			<table>
				<thead>
					<tr>
						<th>Slot</th><th>Velikost</th><th>Běží na</th><th>Modul umí</th><th>Výrobce</th>
						<th>Označení</th>
					</tr>
				</thead>
				<tbody>
					{#each statics.ram_modules as m, i (m.slot + m.part_number + i)}
						<tr>
							<td class="mono">{m.slot}</td>
							<td>{(m.size_mb / 1024).toFixed(0)} GB</td>
							<td>{m.configured_mts || '—'} MT/s</td>
							<td class="dim">{m.speed_mts || '—'} MT/s</td>
							<td>{m.manufacturer || '—'}</td>
							<td class="mono dim">{m.part_number || '—'}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</section>

	<!-- ── Obrazovky ── -->
	{#if displays.length}
		<section class="card">
			<h2><Monitor size={14} /> Obrazovky</h2>
			<table>
				<thead>
					<tr><th>Monitor</th><th>Rozlišení</th><th>Obnovovací frekvence</th><th>Adaptér</th></tr>
				</thead>
				<tbody>
					{#each displays as d, i (d.adapter + i)}
						<tr>
							<td>
								{d.monitor || '—'}
								{#if d.primary}<span class="tag">hlavní</span>{/if}
							</td>
							<td>{d.width} × {d.height}</td>
							<td>{d.refresh_hz} Hz</td>
							<td class="dim">{d.adapter}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</section>
	{/if}

	<!-- ── Úložiště: karta na fyzický disk ── -->
	{#each hw?.disks ?? [] as d (d.index)}
		<section class="card">
			<h2>
				<HardDrive size={14} />
				{d.model || `Disk ${d.index}`}
				{#if d.critical}<span class="tag danger">SMART hlásí problém</span>{/if}
			</h2>
			<dl>
				<div>
					<dt><Thermometer size={11} /> Teplota</dt>
					{#if d.temp_c != null}
						<dd><span class={tempClass(d.temp_c, 55, 70)}>{d.temp_c} °C</span></dd>
					{:else}
						<dd class="dim">nehlásí</dd>
						<p class="note">zdraví přes SMART umí NVMe; tenhle disk ho nedává</p>
					{/if}
				</div>
				<div>
					<dt>Opotřebení</dt>
					{#if d.used_pct != null}
						<dd class={d.used_pct >= 80 ? 'warn' : 'ok'}>{d.used_pct} %</dd>
						<p class="note">návrhové životnosti</p>
					{:else}
						<dd class="dim">nehlásí</dd>
					{/if}
				</div>
				<div>
					<dt>Rezervní bloky</dt>
					<dd class={d.spare_pct == null ? 'dim' : ''}>
						{d.spare_pct != null ? d.spare_pct + ' %' : 'nehlásí'}
					</dd>
				</div>
				<div>
					<dt>Naběháno</dt>
					<dd class={d.power_on_hours == null ? 'dim' : ''}>
						{d.power_on_hours != null ? hours(d.power_on_hours) : 'nehlásí'}
					</dd>
				</div>
			</dl>
			{#each volumesOf(d.index) as v (v.letter)}
				{@const pct = v.total_bytes ? ((v.total_bytes - v.free_bytes) / v.total_bytes) * 100 : 0}
				<div class="vol">
					<div class="vol-head">
						<span class="mono">{v.letter}:</span>
						<span>{v.label || v.fs}</span>
						<span class="dim">{v.fs}</span>
						<span class="vol-num">{bytes(v.free_bytes)} volných z {bytes(v.total_bytes)}</span>
					</div>
					<div class="bar" class:full={pct >= 90}>
						<div class="fill" style="width: {pct}%"></div>
					</div>
				</div>
			{/each}
		</section>
	{/each}

	<!-- ── Baterie: jen u strojů, které ji mají ── -->
	{#if hw?.battery}
		<section class="card">
			<h2><BatteryCharging size={14} /> Baterie</h2>
			<dl>
				<div><dt>Nabití</dt><dd>{hw.battery.percent ?? '—'} %</dd></div>
				<div>
					<dt>Napájení</dt>
					<dd>
						{#if hw.battery.charging}nabíjí se{:else if hw.battery.ac_online}ze sítě{:else}z
							baterie{/if}
					</dd>
					{#if remaining(hw.battery.remaining_s)}
						<p class="note">zbývá asi {remaining(hw.battery.remaining_s)}</p>
					{/if}
				</div>
				<div>
					<dt>Opotřebení</dt>
					{#if hw.battery.wear_pct != null}
						<dd class={hw.battery.wear_pct >= 30 ? 'warn' : 'ok'}>
							{Math.round(hw.battery.wear_pct)} %
						</dd>
						<p class="note">
							nabije se na {(hw.battery.full_mwh / 1000).toFixed(1)} Wh z původních
							{(hw.battery.design_mwh / 1000).toFixed(1)} Wh
						</p>
					{:else}
						<dd class="dim">baterie kapacity nehlásí</dd>
					{/if}
				</div>
				<div>
					<dt>Nabíjecích cyklů</dt>
					<dd class={hw.battery.cycles == null ? 'dim' : ''}>{hw.battery.cycles ?? 'nehlásí'}</dd>
				</div>
			</dl>
		</section>
	{/if}

	<!-- ── Deska a firmware ── -->
	{#if hw?.board}
		<section class="card">
			<h2><CircuitBoard size={14} /> Základní deska</h2>
			<dl>
				<div><dt>Výrobce</dt><dd>{hw.board.manufacturer || '—'}</dd></div>
				<div><dt>Model</dt><dd>{hw.board.product || '—'}</dd></div>
				<div><dt>Revize</dt><dd class="dim">{hw.board.version || '—'}</dd></div>
				<div><dt>BIOS / UEFI</dt><dd>{hw.board.bios_version || '—'}</dd></div>
				<div><dt>Dodavatel BIOSu</dt><dd class="dim">{hw.board.bios_vendor || '—'}</dd></div>
				<div><dt>Datum BIOSu</dt><dd>{hw.board.bios_date || '—'}</dd></div>
				{#if hw.board.system_product}
					<div><dt>Stroj</dt><dd>{hw.board.system_manufacturer} {hw.board.system_product}</dd></div>
				{/if}
			</dl>
		</section>
	{/if}

	<!-- ── Všechna zařízení: úplný soupis, jako Správce zařízení ── -->
	<section class="card">
		<h2>
			<CircuitBoard size={14} /> Všechna zařízení
			<span class="count">{hw?.devices?.length ?? 0}</span>
			{#if problemCount}
				<span class="tag danger">{problemCount} s problémem</span>
			{/if}
		</h2>
		<div class="search">
			<Search size={13} />
			<input placeholder="hledat zařízení, výrobce, třídu…" bind:value={devFilter} />
		</div>
		{#each deviceGroups as g (g.name)}
			{@const open = openClass.has(g.name) || devFilter.trim().length > 0}
			<div class="grp">
				<button class="grp-head" onclick={() => toggleClass(g.name)}>
					<span class="caret" class:open><ChevronRight size={13} /></span>
					<span class="grp-name">{g.name}</span>
					<span class="count">{g.items.length}</span>
					{#if g.problems}<span class="tag danger">{g.problems}</span>{/if}
				</button>
				{#if open}
					<table>
						<thead>
							<tr><th>Zařízení</th><th>Výrobce</th><th>Ovladač</th><th>Datum</th><th>Stav</th></tr>
						</thead>
						<tbody>
							{#each g.items as d, i (d.name + d.hardware_id + i)}
								<tr>
									<td>{d.name}</td>
									<td class="dim">{d.manufacturer || '—'}</td>
									<td class="mono">{d.driver_version || '—'}</td>
									<td class="dim">{d.driver_date || '—'}</td>
									<td>
										{#if d.problem_code}
											<span class="warn"
												><TriangleAlert size={11} /> kód {d.problem_code}</span
											>
										{:else}
											<span class="ok">v pořádku</span>
										{/if}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				{/if}
			</div>
		{/each}
		{#if !deviceGroups.length}
			<p class="empty">
				{devFilter ? 'Nic neodpovídá hledání.' : 'Soupis zařízení se načítá…'}
			</p>
		{/if}
	</section>
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: 12px;
		height: 100%;
		min-height: 0;
		overflow-y: auto;
		padding-right: 4px;
	}

	/* ── Živé dlaždice ── */
	.vitals {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
		gap: 12px;
	}
	.tile {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 10px 12px 12px;
	}
	.tile-head {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 0.72rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--text-dim);
		margin-bottom: 6px;
	}
	.tile-val {
		font-size: 1.6rem;
		font-weight: 600;
		font-variant-numeric: tabular-nums;
		line-height: 1.1;
		margin-bottom: 4px;
	}
	.tile-val small {
		font-size: 0.9rem;
		font-weight: 400;
		color: var(--text-dim);
	}
	.tile-sub,
	.tile-none {
		font-size: 0.75rem;
		color: var(--text-dim);
		margin-top: 4px;
		font-variant-numeric: tabular-nums;
	}
	.tile-none {
		font-variant-numeric: normal;
		line-height: 1.4;
	}

	/* ── Karty komponent ── */
	.card {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 12px 14px 14px;
	}
	.card h2 {
		display: flex;
		align-items: center;
		gap: 7px;
		font-size: 0.88rem;
		font-weight: 600;
		margin: 0 0 12px;
		color: var(--text);
	}

	/* Každý údaj má vlastní buňku s vlastním popiskem — nic se
	   nepřekrývá a nic se nemačká do jednoho řádku. */
	dl {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
		gap: 12px 16px;
		margin: 0;
	}
	dl > div {
		display: flex;
		flex-direction: column;
		gap: 3px;
		min-width: 0;
	}
	dt {
		display: flex;
		align-items: center;
		gap: 4px;
		font-size: 0.68rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--text-dim);
	}
	dd {
		display: flex;
		align-items: center;
		gap: 4px;
		margin: 0;
		font-size: 0.88rem;
		font-variant-numeric: tabular-nums;
	}
	/* Vysvětlivka pod hodnotou — vlastní řádek, ne do ní vražená. */
	.note {
		margin: 0;
		font-size: 0.7rem;
		line-height: 1.35;
		color: var(--text-dim);
	}

	.dim {
		color: var(--text-dim);
	}
	.mono {
		font-family: var(--mono);
		font-size: 0.8rem;
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
	.tag {
		font-size: 0.68rem;
		padding: 1px 6px;
		border-radius: 999px;
		border: 1px solid var(--border);
		color: var(--text-dim);
		font-weight: 400;
		text-transform: none;
		letter-spacing: 0;
	}
	.tag.danger {
		border-color: var(--danger);
		color: var(--danger);
	}
	.count {
		font-size: 0.72rem;
		color: var(--text-dim);
		font-variant-numeric: tabular-nums;
	}

	table {
		width: 100%;
		border-collapse: collapse;
		margin-top: 12px;
		font-size: 0.8rem;
	}
	th {
		text-align: left;
		font-weight: 500;
		font-size: 0.68rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--text-dim);
		padding: 5px 10px 5px 0;
		border-bottom: 1px solid var(--border);
		white-space: nowrap;
	}
	td {
		padding: 6px 10px 6px 0;
		border-bottom: 1px solid var(--border);
		vertical-align: top;
	}
	td:last-child,
	th:last-child {
		padding-right: 0;
	}

	/* ── Svazky pod diskem ── */
	.vol {
		margin-top: 12px;
	}
	.vol-head {
		display: flex;
		align-items: baseline;
		gap: 8px;
		font-size: 0.8rem;
		margin-bottom: 5px;
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

	/* ── Soupis zařízení ── */
	.search {
		display: flex;
		align-items: center;
		gap: 7px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 5px 9px;
		margin-bottom: 4px;
		color: var(--text-dim);
	}
	.search input {
		flex: 1;
		background: none;
		border: none;
		outline: none;
		color: var(--text);
		font: inherit;
		font-size: 0.8rem;
	}
	.grp {
		border-bottom: 1px solid var(--border);
	}
	.grp:last-child {
		border-bottom: none;
	}
	.grp-head {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		background: none;
		border: none;
		color: var(--text);
		font: inherit;
		font-size: 0.82rem;
		text-align: left;
		padding: 9px 2px;
		cursor: pointer;
	}
	.grp-head:hover {
		color: var(--accent);
	}
	.caret {
		display: grid;
		place-items: center;
		color: var(--text-dim);
		transition: transform 0.12s ease;
	}
	.caret.open {
		transform: rotate(90deg);
	}
	.grp-name {
		flex: 1;
		min-width: 0;
	}
	.grp table {
		margin-top: 0;
		margin-bottom: 10px;
	}

	.empty {
		color: var(--text-dim);
		font-size: 0.82rem;
		padding: 8px 0;
	}
</style>
