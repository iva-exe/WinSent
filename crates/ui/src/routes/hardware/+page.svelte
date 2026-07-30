<script>
	// Hardware (v9, SPEC kap. 15) — soupis všeho, co v počítači je.
	//
	// Jeden plochý seznam. Žádné grafy (od toho je Tasks), žádné
	// rozbalování — všechno je vidět hned. Celá stránka je JEDNA mřížka,
	// takže název, výrobce a stav sedí ve stejném sloupci u procesoru
	// i u tiskárny. Prostřední sloupec je volný prostor: každý typ
	// zařízení tam dá to, co dává smysl u něj.
	//
	// Řazení podle důležitosti: komponenty → zobrazení → periferie →
	// zvuk → síť → řadiče → tisk → systémová zařízení.
	//
	// Pravidlo ze SPEC 15.2: nikdy nepředstírat číslo, které nemáme.
	// U teploty se vždy ukazuje zdroj; když ji nikdo nehlásí, řekne se
	// to nahlas.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { Search, TriangleAlert } from 'lucide-svelte';

	let statics = $state(null);
	let hw = $state(null);
	let sys = $state(null);
	// Obrazovky čte UI proces, ne služba — ta běží v session 0, kde
	// žádná plocha není a seznam by byl prázdný.
	let displays = $state([]);
	let loadError = $state('');
	let filter = $state('');

	async function loadHw() {
		try {
			hw = await invoke('query_hardware');
			loadError = '';
		} catch (e) {
			loadError = String(e);
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
		const t1 = setInterval(
			() =>
				invoke('query_system')
					.then((s) => (sys = s))
					.catch(() => {}),
			2000
		);
		invoke('query_system')
			.then((s) => (sys = s))
			.catch(() => {});
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
		if (h == null) return null;
		if (h >= 8760) return (h / 8760).toFixed(1) + ' roku provozu';
		if (h >= 24) return Math.round(h / 24) + ' dní provozu';
		return h + ' h provozu';
	}

	// Ovladač jako jeden údaj do volného sloupce.
	function driver(d) {
		if (!d.driver_version) return null;
		return d.driver_date
			? `ovladač ${d.driver_version} z ${d.driver_date}`
			: `ovladač ${d.driver_version}`;
	}

	// Hardwarové ID zkrácené na část, která identifikuje model.
	function hwid(d) {
		if (!d.hardware_id) return null;
		const s = d.hardware_id.split('&').slice(0, 2).join('&');
		return s.length > 46 ? s.slice(0, 46) + '…' : s;
	}

	// ── Kategorie: hrubé dělení podle důležitosti, ne podle tříd
	// Windows. Uživatele nezajímá, že klávesnice je „HIDClass" —
	// zajímá ho, že je to periferie.
	const CATEGORIES = [
		{ name: 'Zobrazení', classes: ['Monitor'] },
		{
			name: 'Periferie',
			classes: ['Keyboard', 'Mouse', 'HIDClass', 'WPD', 'Image', 'Camera', 'Bluetooth', 'Biometric']
		},
		{ name: 'Zvuková zařízení', classes: ['MEDIA', 'AudioEndpoint', 'AudioProcessingObject'] },
		{ name: 'Síť', classes: ['Net'] },
		{
			name: 'Řadiče a porty',
			classes: ['USB', 'HDC', 'SCSIAdapter', 'Ports', 'Volume', 'DiskDrive', 'FloppyDisk']
		},
		{ name: 'Tisk', classes: ['PrintQueue', 'Printer', 'PrinterPort'] },
		{
			name: 'Systémová zařízení',
			classes: ['System', 'Computer', 'Firmware', 'SoftwareDevice', 'SecurityDevices', 'Processor']
		}
	];

	// Výrobci si zakládají vlastní třídy („Focusrite Audio", „Razer
	// Device"), takže samotný seznam tříd nestačí. Když třídu neznáme,
	// rozhodne její název a pak sběrnice v hardwarovém ID: co visí na
	// HID nebo USB, je z pohledu uživatele periferie.
	function categoryOf(dev) {
		for (const c of CATEGORIES) {
			if (c.classes.includes(dev.class)) return c.name;
		}
		const cls = (dev.class + ' ' + dev.class_desc).toLowerCase();
		if (cls.includes('audio') || cls.includes('zvuk')) return 'Zvuková zařízení';
		if (cls.includes('net') || cls.includes('síť')) return 'Síť';
		// Vlastní sběrnice výrobců (RAZER\, RZCONTROL\…) mají pořád
		// VID/PID — je to zařízení pořízené přes USB, tedy periferie.
		const bus = (dev.hardware_id || '').toUpperCase();
		if (bus.includes('VID_') && bus.includes('PID_')) return 'Periferie';
		return 'Ostatní zařízení';
	}

	function matches(text) {
		const q = filter.trim().toLowerCase();
		return !q || text.toLowerCase().includes(q);
	}

	// Zařízení rozdělená do kategorií, v pořadí důležitosti.
	let deviceSections = $derived.by(() => {
		const order = [...CATEGORIES.map((c) => c.name), 'Ostatní zařízení'];
		const map = new Map(order.map((n) => [n, []]));
		for (const d of hw?.devices ?? []) {
			// Procesor, grafika a disky mají vlastní řádky nahoře
			// v komponentách — níž by se jen opakovaly.
			if (d.class === 'Processor' || d.class === 'DiskDrive' || d.class === 'Display') continue;
			if (!matches(`${d.name} ${d.manufacturer} ${d.class_desc} ${d.class}`)) continue;
			map.get(categoryOf(d))?.push(d);
		}
		return order
			.map((name) => ({ name, items: map.get(name) ?? [] }))
			.filter((s) => s.items.length);
	});

	let problemCount = $derived((hw?.devices ?? []).filter((d) => d.problem_code !== 0).length);
	let ramPct = $derived(
		sys?.mem_total_mb ? Math.round((sys.mem_used_mb / sys.mem_total_mb) * 100) : null
	);
	// Grafik může být víc (integrovaná + dedikovaná). Živá telemetrie
	// je jen od té, kterou umíme číst — ostatní se vypíšou bez ní,
	// ne s vymyšlenými nulami.
	let gpuDevices = $derived((hw?.devices ?? []).filter((d) => d.class === 'Display'));

	function isLiveGpu(dev) {
		const n = statics?.gpu_name;
		if (!n || sys?.gpu_pct == null) return false;
		const a = dev.name.toLowerCase();
		const b = n.toLowerCase();
		return a.includes(b) || b.includes(a);
	}
	let cpuVendor = $derived(
		(hw?.devices ?? []).find((d) => d.class === 'Processor')?.manufacturer ?? '—'
	);

	// Výrobce disku ze stromu zařízení — SMART hlásí jen model.
	function diskVendor(model) {
		if (!model) return '—';
		const key = model.split(' ')[0].toLowerCase();
		const dev = (hw?.devices ?? []).find(
			(d) => d.class === 'DiskDrive' && d.name.toLowerCase().includes(key)
		);
		return dev?.manufacturer || '—';
	}
	let netCount = $derived((hw?.devices ?? []).filter((d) => d.class === 'Net').length);
</script>

<div class="page">
	<header class="head">
		<div class="search">
			<Search size={13} />
			<input placeholder="hledat zařízení, výrobce…" bind:value={filter} />
		</div>
		<span class="head-count">
			{hw?.devices?.length ?? 0} zařízení
			{#if problemCount}
				<span class="bad">· {problemCount} s problémem</span>
			{/if}
		</span>
	</header>

	{#if loadError}
		<p class="empty">Nelze načíst hardware: {loadError}</p>
	{/if}

	<!-- Celá stránka je jedna mřížka: sloupce sedí napříč všemi
	     sekcemi, od procesoru po tiskárnu. -->
	<div class="grid">
		<div class="colhead">
			<span>Zařízení</span><span>Výrobce</span><span>Podrobnosti</span><span>Stav</span>
		</div>

		<!-- ── 1. Komponenty ── -->
		<h2 class="sect">Komponenty</h2>

		{#if matches(`procesor cpu ${statics?.cpu_name ?? ''}`)}
			<div class="row">
				<div class="c-name">{statics?.cpu_name ?? 'Procesor'}</div>
				<div class="c-vendor">{cpuVendor}</div>
				<div class="c-detail">
					<span>{statics?.physical_cores ?? '—'} fyzických / {statics?.logical_cores ?? '—'} logických jader</span>
					<span>{hw?.cpu_thermal?.clock_mhz ?? '—'} MHz z {hw?.cpu_thermal?.max_mhz ?? '—'} MHz</span>
					<span class="dim">L1 {statics?.l1_kb ?? '—'} kB · L2 {statics?.l2_kb ?? '—'} kB · L3 {statics?.l3_kb ?? '—'} kB</span>
					{#if hw?.cpu_thermal?.celsius != null}
						<span>{Math.round(hw.cpu_thermal.celsius)} °C <span class="dim">(zdroj: {hw.cpu_thermal.temp_source})</span></span>
					{:else}
						<span class="dim">teplotu tenhle stroj z Windows nehlásí — ukázala by se, kdyby běžel HWiNFO nebo LibreHardwareMonitor</span>
					{/if}
				</div>
				<div class="c-state">
					<span class="num">{sys ? Math.round(sys.cpu_pct) : '—'} %</span>
					{#if hw?.cpu_thermal?.throttling}
						<span class="warn">běží pod maximem</span>
					{:else}
						<span class="ok">jede naplno</span>
					{/if}
				</div>
			</div>
		{/if}

		{#if matches('paměť ram')}
			<div class="row">
				<div class="c-name">Paměť</div>
				<div class="c-vendor">{statics?.ram_modules?.[0]?.manufacturer ?? '—'}</div>
				<div class="c-detail">
					<span>{gb(sys?.mem_total_mb)} celkem</span>
					<span>{statics?.ram_modules?.length ?? 0} modulů ve {statics?.ram_slots ?? '—'} slotech</span>
					{#each statics?.ram_modules ?? [] as m, i (m.slot + i)}
						<span class="dim">
							{m.slot}: {(m.size_mb / 1024).toFixed(0)} GB @ {m.configured_mts || '—'} MT/s
							(modul umí {m.speed_mts || '—'}) · {m.part_number || '—'}
						</span>
					{/each}
				</div>
				<div class="c-state">
					<span class="num">{ramPct ?? '—'} %</span>
					<span class="dim">{gb(sys?.mem_used_mb)} z {gb(sys?.mem_total_mb)}</span>
				</div>
			</div>
		{/if}

		{#each gpuDevices as g, i (g.name + i)}
			{#if matches(`${g.name} ${g.manufacturer} grafika`)}
				{@const live = isLiveGpu(g)}
				<div class="row">
					<div class="c-name">{g.name}</div>
					<div class="c-vendor">{g.manufacturer || '—'}</div>
					<div class="c-detail">
						{#if driver(g)}<span>{driver(g)}</span>{/if}
						{#if live && sys?.gpu?.vram_used_mb != null}
							<span>VRAM {gb(sys.gpu.vram_used_mb)} z {gb(sys.gpu.vram_total_mb)}</span>
						{/if}
						{#if live && sys?.gpu?.clock_mhz != null}<span>{sys.gpu.clock_mhz} MHz</span>{/if}
						{#if live && sys?.gpu?.power_w != null}<span>{Math.round(sys.gpu.power_w)} W</span>{/if}
						{#if !live}
							<span class="dim">zatížení ani teplotu tahle karta přes ovladač nehlásí</span>
						{/if}
						{#if hwid(g)}<span class="mono dim">{hwid(g)}</span>{/if}
					</div>
					<div class="c-state">
						{#if live}
							<span class="num">{Math.round(sys.gpu_pct)} %</span>
						{/if}
						{#if live && sys?.gpu?.temp_c != null}
							<span class={sys.gpu.temp_c >= 88 ? 'hot' : sys.gpu.temp_c >= 75 ? 'warm' : 'cool'}>
								{Math.round(sys.gpu.temp_c)} °C
							</span>
						{:else if g.problem_code}
							<span class="bad"><TriangleAlert size={11} /> problém {g.problem_code}</span>
						{:else}
							<span class="ok">v pořádku</span>
						{/if}
					</div>
				</div>
			{/if}
		{/each}

		{#each hw?.disks ?? [] as d (d.index)}
			{#if matches(`${d.model} disk`)}
				<div class="row">
					<div class="c-name">{d.model || `Disk ${d.index}`}</div>
					<div class="c-vendor">{diskVendor(d.model)}</div>
					<div class="c-detail">
						{#each (hw?.volumes ?? []).filter((v) => v.disk_index === d.index) as v (v.letter)}
							<span>
								{v.letter}: {v.label || v.fs} — {bytes(v.free_bytes)} volných z {bytes(v.total_bytes)}
							</span>
						{/each}
						{#if d.power_on_hours != null}<span class="dim">{hours(d.power_on_hours)}</span>{/if}
						{#if d.spare_pct != null}<span class="dim">rezervní bloky {d.spare_pct} %</span>{/if}
						{#if d.temp_c == null && d.used_pct == null}
							<span class="dim">zdraví přes SMART umí NVMe disky; tenhle ho nedává</span>
						{/if}
					</div>
					<div class="c-state">
						{#if d.temp_c != null}
							<span class={d.temp_c >= 70 ? 'hot' : d.temp_c >= 55 ? 'warm' : 'cool'}>
								{d.temp_c} °C
							</span>
						{/if}
						{#if d.used_pct != null}
							<span class={d.used_pct >= 80 ? 'warn' : 'dim'}>opotřebení {d.used_pct} %</span>
						{/if}
						{#if d.critical}
							<span class="bad"><TriangleAlert size={11} /> SMART hlásí problém</span>
						{:else if d.temp_c == null && d.used_pct == null}
							<span class="dim">nehlásí</span>
						{:else}
							<span class="ok">v pořádku</span>
						{/if}
					</div>
				</div>
			{/if}
		{/each}

		{#if hw?.board && matches(`deska ${hw.board.manufacturer} ${hw.board.product} bios`)}
			<div class="row">
				<div class="c-name">{hw.board.product || 'Základní deska'}</div>
				<div class="c-vendor">{hw.board.manufacturer || '—'}</div>
				<div class="c-detail">
					{#if hw.board.version}<span>revize {hw.board.version}</span>{/if}
					<span>BIOS {hw.board.bios_version || '—'} z {hw.board.bios_date || '—'}</span>
					<span class="dim">{hw.board.bios_vendor}</span>
					{#if hw.board.system_product}
						<span class="dim">stroj: {hw.board.system_manufacturer} {hw.board.system_product}</span>
					{/if}
				</div>
				<div class="c-state"><span class="ok">v pořádku</span></div>
			</div>
		{/if}

		{#if hw?.battery && matches('baterie')}
			<div class="row">
				<div class="c-name">Baterie</div>
				<div class="c-vendor">—</div>
				<div class="c-detail">
					<span>
						{#if hw.battery.charging}nabíjí se{:else if hw.battery.ac_online}napájení ze sítě{:else}běží z baterie{/if}
					</span>
					{#if hw.battery.wear_pct != null}
						<span>
							nabije se na {(hw.battery.full_mwh / 1000).toFixed(1)} Wh z původních
							{(hw.battery.design_mwh / 1000).toFixed(1)} Wh
						</span>
					{/if}
					{#if hw.battery.cycles != null}<span class="dim">{hw.battery.cycles} nabíjecích cyklů</span>{/if}
				</div>
				<div class="c-state">
					<span class="num">{hw.battery.percent ?? '—'} %</span>
					{#if hw.battery.wear_pct != null}
						<span class={hw.battery.wear_pct >= 30 ? 'warn' : 'ok'}>
							opotřebení {Math.round(hw.battery.wear_pct)} %
						</span>
					{:else}
						<span class="dim">kapacity nehlásí</span>
					{/if}
				</div>
			</div>
		{/if}

		<!-- ── 2. Obrazovky (režim čte UI ve své relaci) ── -->
		{#if displays.length}
			<h2 class="sect">Obrazovky</h2>
			{#each displays as d, i (d.adapter + i)}
				{#if matches(`${d.monitor} ${d.adapter} monitor obrazovka`)}
					<div class="row">
						<div class="c-name">{d.monitor || 'Obrazovka'}</div>
						<div class="c-vendor">{d.adapter}</div>
						<div class="c-detail">
							<span>{d.width} × {d.height} bodů</span>
							<span>{d.refresh_hz} Hz</span>
						</div>
						<div class="c-state">
							{#if d.primary}<span class="num">hlavní</span>{/if}
							<span class="ok">připojená</span>
						</div>
					</div>
				{/if}
			{/each}
		{/if}

		<!-- ── 3.–8. Ostatní zařízení podle kategorií ── -->
		{#each deviceSections as sect (sect.name)}
			<h2 class="sect">
				{sect.name}
				<span class="sect-count">{sect.items.length}</span>
			</h2>
			{#each sect.items as d, i (d.name + d.hardware_id + i)}
				<div class="row">
					<div class="c-name">{d.name}</div>
					<div class="c-vendor">{d.manufacturer || '—'}</div>
					<div class="c-detail">
						{#if driver(d)}<span>{driver(d)}</span>{/if}
						{#if d.class_desc}<span class="dim">{d.class_desc}</span>{/if}
						{#if hwid(d)}<span class="mono dim">{hwid(d)}</span>{/if}
					</div>
					<div class="c-state">
						{#if d.problem_code}
							<span class="bad"><TriangleAlert size={11} /> problém {d.problem_code}</span>
						{:else}
							<span class="ok">v pořádku</span>
						{/if}
					</div>
				</div>
			{/each}
		{/each}

		{#if !deviceSections.length && filter}
			<p class="empty">Nic neodpovídá hledání.</p>
		{/if}
	</div>

	{#if netCount}
		<p class="foot">
			Síťová spojení, porty a provoz najdeš v sekci Network — tady jsou jen adaptéry.
		</p>
	{/if}
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: 10px;
		height: 100%;
		min-height: 0;
		overflow-y: auto;
		padding-right: 4px;
	}

	.head {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.search {
		display: flex;
		align-items: center;
		gap: 7px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 5px 9px;
		color: var(--text-dim);
		flex: 1;
		max-width: 320px;
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
	.head-count {
		margin-left: auto;
		font-size: 0.78rem;
		color: var(--text-dim);
		font-variant-numeric: tabular-nums;
	}

	/* Jedna mřížka pro celou stránku — díky tomu sedí sloupce
	   u procesoru stejně jako u tiskárny. Prostřední sloupec je
	   volný prostor pro to, co dává smysl u daného zařízení. */
	.grid {
		display: grid;
		grid-template-columns:
			minmax(200px, 1.5fr)
			minmax(120px, 0.9fr)
			minmax(240px, 2.2fr)
			minmax(130px, 0.8fr);
		align-items: start;
		column-gap: 16px;
	}
	.colhead,
	.row {
		display: grid;
		grid-column: 1 / -1;
		grid-template-columns: subgrid;
		padding: 7px 2px;
		border-bottom: 1px solid var(--border);
	}
	.colhead {
		position: sticky;
		top: 0;
		z-index: 2;
		background: var(--bg);
		font-size: 0.66rem;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-dim);
		padding-bottom: 5px;
	}
	.row:hover {
		background: var(--surface);
	}

	/* Nadpis sekce přes celou šířku — nerozbíjí zarovnání sloupců. */
	.sect {
		grid-column: 1 / -1;
		display: flex;
		align-items: baseline;
		gap: 8px;
		margin: 18px 0 4px;
		font-size: 0.75rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		color: var(--text-dim);
		border-bottom: 1px solid var(--border-strong, var(--border));
		padding-bottom: 5px;
	}
	.sect:first-of-type {
		margin-top: 10px;
	}
	.sect-count {
		font-weight: 400;
		letter-spacing: 0;
		font-variant-numeric: tabular-nums;
	}

	.c-name {
		font-size: 0.83rem;
		line-height: 1.35;
		word-break: break-word;
	}
	.c-vendor {
		font-size: 0.78rem;
		color: var(--text-dim);
		line-height: 1.35;
		word-break: break-word;
	}
	/* Volný sloupec: každý údaj vlastní řádek, ať se nic nemačká. */
	.c-detail,
	.c-state {
		display: flex;
		flex-direction: column;
		gap: 2px;
		font-size: 0.78rem;
		line-height: 1.4;
		min-width: 0;
	}
	.c-detail span {
		word-break: break-word;
	}
	.c-state {
		align-items: flex-end;
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	/* Aktuální vytížení je jen číslo — grafy má Tasks. */
	.num {
		font-size: 0.95rem;
		font-weight: 600;
	}

	.dim {
		color: var(--text-dim);
	}
	.mono {
		font-family: var(--mono);
		font-size: 0.72rem;
	}
	.ok {
		color: var(--ok);
	}
	.warn {
		color: var(--warn);
	}
	.bad {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--danger);
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

	.empty {
		grid-column: 1 / -1;
		color: var(--text-dim);
		font-size: 0.82rem;
		padding: 14px 0;
	}
	.foot {
		font-size: 0.75rem;
		color: var(--text-dim);
		padding: 4px 2px 12px;
	}
</style>
