<script>
	// Hardware (v9, SPEC kap. 15) — soupis všeho, co v počítači je.
	//
	// Rozvržení:
	//  • hlavička je pevná (nescrolluje) — hledání, přepínač kategorií
	//    a upozornění na problémy jsou pořád po ruce,
	//  • tělo je vlastní seznam karet, ne tabulka: ikona, název (větší
	//    než zbytek), výrobce a pod tím fakta jako štítky,
	//  • vpravo stav — jedno číslo a jedno slovo. Grafy tu nejsou,
	//    od toho je Tasks.
	//
	// Kategorie jdou shora podle důležitosti: komponenty → obrazovky →
	// periferie → zvuk → síť → řadiče → tisk → systémová zařízení.
	//
	// Pravidlo ze SPEC 15.2: nikdy nepředstírat číslo, které nemáme —
	// u teploty se vždy ukazuje zdroj, jinak se řekne, že chybí.
	import { onMount, tick as nextTick } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import {
		AudioLines,
		BatteryCharging,
		Bluetooth,
		Boxes,
		Cable,
		ChevronRight,
		CircuitBoard,
		Cog,
		Component,
		Cpu,
		Disc,
		Gamepad2,
		HardDrive,
		Headphones,
		Keyboard,
		MemoryStick,
		Microchip,
		Monitor,
		Mouse,
		Network,
		Plug,
		Printer,
		Search,
		Server,
		Shield,
		Smartphone,
		TriangleAlert,
		Usb,
		Webcam
	} from 'lucide-svelte';

	let statics = $state(null);
	let hw = $state(null);
	let sys = $state(null);
	// Obrazovky čte UI proces, ne služba — ta běží v session 0, kde
	// žádná plocha není a seznam by byl prázdný.
	let displays = $state([]);
	let loadError = $state('');
	let filter = $state('');

	let bodyEl = $state(null);
	let activeCat = $state('Komponenty');
	let flashId = $state('');
	let problemIdx = $state(-1);

	async function loadHw() {
		try {
			hw = await invoke('query_hardware');
			loadError = '';
		} catch (e) {
			loadError = String(e);
		}
	}

	async function loadSys() {
		try {
			sys = await invoke('query_system');
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
		loadSys();
		const t1 = setInterval(loadSys, 2000);
		// Tepelná kaskáda sahá na WMI a soupis zařízení na SetupAPI —
		// po sekundách, ne v cyklu (SPEC 15.2). Služba to ještě cachuje.
		const t2 = setInterval(loadHw, 8000);
		return () => {
			clearInterval(t1);
			clearInterval(t2);
		};
	});

	// ── Formátování ──
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

	function driver(d) {
		if (!d.driver_version) return null;
		return d.driver_date
			? `ovladač ${d.driver_version} · ${d.driver_date}`
			: `ovladač ${d.driver_version}`;
	}

	// Hardwarové ID zkrácené na část, která identifikuje model.
	function hwid(d) {
		if (!d.hardware_id) return null;
		const s = d.hardware_id.split('&').slice(0, 2).join('&');
		return s.length > 44 ? s.slice(0, 44) + '…' : s;
	}

	// ── Kategorie: hrubé dělení podle toho, co zařízení znamená pro
	// uživatele — ne podle tříd Windows. Nikoho nezajímá, že klávesnice
	// je „HIDClass".
	const CATEGORIES = [
		{ name: 'Komponenty', icon: Cpu },
		{ name: 'Obrazovky', icon: Monitor },
		{ name: 'Periferie', icon: Keyboard },
		{ name: 'Zvuk', icon: AudioLines },
		{ name: 'Síť', icon: Network },
		{ name: 'Řadiče a porty', icon: Usb },
		{ name: 'Tisk', icon: Printer },
		{ name: 'Systémová zařízení', icon: Cog },
		{ name: 'Ostatní', icon: Boxes }
	];

	const CLASS_CATEGORY = {
		Monitor: 'Obrazovky',
		Keyboard: 'Periferie',
		Mouse: 'Periferie',
		HIDClass: 'Periferie',
		WPD: 'Periferie',
		Image: 'Periferie',
		Camera: 'Periferie',
		Bluetooth: 'Periferie',
		Biometric: 'Periferie',
		MEDIA: 'Zvuk',
		AudioEndpoint: 'Zvuk',
		AudioProcessingObject: 'Zvuk',
		Net: 'Síť',
		USB: 'Řadiče a porty',
		HDC: 'Řadiče a porty',
		SCSIAdapter: 'Řadiče a porty',
		Ports: 'Řadiče a porty',
		Volume: 'Řadiče a porty',
		FloppyDisk: 'Řadiče a porty',
		PrintQueue: 'Tisk',
		Printer: 'Tisk',
		PrinterPort: 'Tisk',
		System: 'Systémová zařízení',
		Computer: 'Systémová zařízení',
		Firmware: 'Systémová zařízení',
		SoftwareDevice: 'Systémová zařízení',
		SecurityDevices: 'Systémová zařízení'
	};

	function categoryOf(dev) {
		const known = CLASS_CATEGORY[dev.class];
		if (known) return known;
		// Výrobci si zakládají vlastní třídy („Focusrite Audio",
		// „Razer Device"), takže seznam tříd nestačí.
		const cls = (dev.class + ' ' + dev.class_desc).toLowerCase();
		if (cls.includes('audio') || cls.includes('zvuk')) return 'Zvuk';
		if (cls.includes('net') || cls.includes('síť')) return 'Síť';
		// Vlastní sběrnice (RAZER\, RZCONTROL\…) mají pořád VID/PID —
		// je to zařízení pořízené přes USB, tedy periferie.
		const bus = (dev.hardware_id || '').toUpperCase();
		if (bus.includes('VID_') && bus.includes('PID_')) return 'Periferie';
		return 'Ostatní';
	}

	const CLASS_ICON = {
		Processor: Cpu,
		Display: Microchip,
		DiskDrive: HardDrive,
		Monitor: Monitor,
		Keyboard: Keyboard,
		Mouse: Mouse,
		HIDClass: Gamepad2,
		MEDIA: AudioLines,
		AudioEndpoint: Headphones,
		Net: Network,
		USB: Usb,
		HDC: Server,
		SCSIAdapter: Server,
		Ports: Plug,
		Volume: Disc,
		PrintQueue: Printer,
		System: Cog,
		Computer: Server,
		Firmware: Microchip,
		SoftwareDevice: Component,
		SecurityDevices: Shield,
		WPD: Smartphone,
		Image: Webcam,
		Camera: Webcam,
		Bluetooth: Bluetooth,
		Biometric: Shield
	};

	function iconOf(dev) {
		if (CLASS_ICON[dev.class]) return CLASS_ICON[dev.class];
		const cat = categoryOf(dev);
		if (cat === 'Zvuk') return AudioLines;
		if (cat === 'Síť') return Network;
		if (cat === 'Periferie') return Gamepad2;
		if (cat === 'Řadiče a porty') return Cable;
		return Component;
	}

	function matches(text) {
		const q = filter.trim().toLowerCase();
		return !q || text.toLowerCase().includes(q);
	}

	// ── Komponenty: bohatší řádky, které se skládají z víc zdrojů ──
	let cpuVendor = $derived(
		(hw?.devices ?? []).find((d) => d.class === 'Processor')?.manufacturer ?? '—'
	);
	let gpuDevices = $derived((hw?.devices ?? []).filter((d) => d.class === 'Display'));
	let ramPct = $derived(
		sys?.mem_total_mb ? Math.round((sys.mem_used_mb / sys.mem_total_mb) * 100) : null
	);

	// Živá telemetrie patří jen té kartě, kterou umíme číst — ostatní
	// dostanou poznámku, ne vymyšlené nuly.
	function isLiveGpu(dev) {
		const n = statics?.gpu_name;
		if (!n || sys?.gpu_pct == null) return false;
		const a = dev.name.toLowerCase();
		const b = n.toLowerCase();
		return a.includes(b) || b.includes(a);
	}

	function diskVendor(model) {
		if (!model) return '—';
		const key = model.split(' ')[0].toLowerCase();
		return (
			(hw?.devices ?? []).find(
				(d) => d.class === 'DiskDrive' && d.name.toLowerCase().includes(key)
			)?.manufacturer || '—'
		);
	}

	function volumesOf(index) {
		return (hw?.volumes ?? []).filter((v) => v.disk_index === index);
	}

	function tempClass(c, warn, hot) {
		return c >= hot ? 'hot' : c >= warn ? 'warm' : 'cool';
	}

	// Zařízení rozdělená do kategorií, v pořadí důležitosti. Procesor,
	// grafika a disky se přeskočí — mají vlastní bohatší karty nahoře.
	let deviceSections = $derived.by(() => {
		const map = new Map(CATEGORIES.map((c) => [c.name, []]));
		for (const d of hw?.devices ?? []) {
			if (d.class === 'Processor' || d.class === 'DiskDrive' || d.class === 'Display') continue;
			if (!matches(`${d.name} ${d.manufacturer} ${d.class_desc} ${d.class}`)) continue;
			map.get(categoryOf(d))?.push(d);
		}
		return map;
	});

	// Které sekce se vůbec vykreslí (a v jakém pořadí) — podle toho se
	// staví přepínač kategorií i cyklení mezi problémy.
	let sections = $derived.by(() => {
		const out = [];
		if (componentRows.length) out.push({ name: 'Komponenty', icon: Cpu, count: componentRows.length });
		const disp = displays.filter((d) => matches(`${d.monitor} ${d.adapter} monitor obrazovka`));
		if (disp.length) out.push({ name: 'Obrazovky', icon: Monitor, count: disp.length });
		for (const c of CATEGORIES) {
			if (c.name === 'Komponenty' || c.name === 'Obrazovky') continue;
			const items = deviceSections.get(c.name) ?? [];
			if (items.length) out.push({ name: c.name, icon: c.icon, count: items.length });
		}
		return out;
	});

	let visibleDisplays = $derived(
		displays.filter((d) => matches(`${d.monitor} ${d.adapter} monitor obrazovka`))
	);

	// ── Komponentové karty jako data, ať jde spočítat jejich počet
	// i jejich problémy dřív, než se vykreslí.
	let componentRows = $derived.by(() => {
		const rows = [];
		if (statics && matches(`procesor cpu ${statics.cpu_name ?? ''}`)) {
			rows.push({ id: 'cmp-cpu', kind: 'cpu' });
		}
		if (matches('paměť ram')) rows.push({ id: 'cmp-ram', kind: 'ram' });
		gpuDevices.forEach((g, i) => {
			if (matches(`${g.name} ${g.manufacturer} grafika gpu`)) {
				rows.push({ id: `cmp-gpu-${i}`, kind: 'gpu', dev: g, problem: g.problem_code !== 0 });
			}
		});
		(hw?.disks ?? []).forEach((d) => {
			if (matches(`${d.model} disk`)) {
				rows.push({ id: `cmp-disk-${d.index}`, kind: 'disk', disk: d, problem: !!d.critical });
			}
		});
		if (hw?.board && matches(`deska ${hw.board.manufacturer} ${hw.board.product} bios`)) {
			rows.push({ id: 'cmp-board', kind: 'board' });
		}
		if (hw?.battery && matches('baterie')) rows.push({ id: 'cmp-battery', kind: 'battery' });
		return rows;
	});

	// ── Problémy: seznam v pořadí vykreslení, ať se dá cyklovat ──
	let problems = $derived.by(() => {
		const out = [];
		for (const r of componentRows) {
			if (r.problem) out.push({ id: r.id, label: r.dev?.name ?? r.disk?.model ?? 'komponenta' });
		}
		for (const s of sections) {
			if (s.name === 'Komponenty' || s.name === 'Obrazovky') continue;
			(deviceSections.get(s.name) ?? []).forEach((d, i) => {
				if (d.problem_code) out.push({ id: `dev-${s.name}-${i}`, label: d.name });
			});
		}
		return out;
	});

	// Skok na další problém; při opakovaném kliknutí cykluje.
	async function jumpToProblem() {
		if (!problems.length) return;
		problemIdx = (problemIdx + 1) % problems.length;
		const target = problems[problemIdx];
		await nextTick();
		const el = document.getElementById(target.id);
		if (!el) return;
		el.scrollIntoView({ behavior: 'smooth', block: 'center' });
		flashId = target.id;
		setTimeout(() => {
			if (flashId === target.id) flashId = '';
		}, 2200);
	}

	function gotoSection(name) {
		const el = document.getElementById('sect-' + name);
		if (!el || !bodyEl) return;
		bodyEl.scrollTo({ top: el.offsetTop - 6, behavior: 'smooth' });
		activeCat = name;
	}

	// Aktivní kategorie podle pozice scrollu — přepínač se zvýrazňuje sám.
	let rafPending = false;
	function onScroll() {
		if (rafPending || !bodyEl) return;
		rafPending = true;
		requestAnimationFrame(() => {
			rafPending = false;
			const y = bodyEl.scrollTop + 12;
			let current = sections[0]?.name ?? '';
			for (const s of sections) {
				const el = document.getElementById('sect-' + s.name);
				if (el && el.offsetTop <= y) current = s.name;
			}
			activeCat = current;
		});
	}
</script>

<div class="page">
	<!-- ── Pevná hlavička: nescrolluje pryč ── -->
	<header class="head">
		<div class="head-top">
			<div class="search">
				<Search size={14} />
				<input placeholder="hledat zařízení nebo výrobce…" bind:value={filter} />
				{#if filter}
					<button class="clear" onclick={() => (filter = '')}>×</button>
				{/if}
			</div>
			<span class="total">
				<strong>{hw?.devices?.length ?? 0}</strong> zařízení
			</span>
			{#if problems.length}
				<button class="alarm" onclick={jumpToProblem}>
					<TriangleAlert size={13} />
					{problems.length}
					{problems.length === 1 ? 'problém' : problems.length < 5 ? 'problémy' : 'problémů'}
					<span class="alarm-go">
						{problemIdx >= 0 ? `${problemIdx + 1}/${problems.length}` : 'ukázat'}
						<ChevronRight size={12} />
					</span>
				</button>
			{/if}
		</div>
		<nav class="cats">
			{#each sections as s (s.name)}
				<button class="cat" class:on={activeCat === s.name} onclick={() => gotoSection(s.name)}>
					<s.icon size={13} />
					{s.name}
					<span class="cat-n">{s.count}</span>
				</button>
			{/each}
		</nav>
	</header>

	<!-- ── Tělo: jediná scrollovaná oblast ── -->
	<div class="body" bind:this={bodyEl} onscroll={onScroll}>
		{#if loadError}
			<p class="empty">Nelze načíst hardware: {loadError}</p>
		{/if}

		{#if componentRows.length}
			<h2 class="sect" id="sect-Komponenty"><Cpu size={14} /> Komponenty</h2>
			{#each componentRows as r (r.id)}
				<article class="item" id={r.id} class:flash={flashId === r.id} class:bad={r.problem}>
					{#if r.kind === 'cpu'}
						<div class="ico"><Cpu size={17} /></div>
						<div class="info">
							<h3>{statics?.cpu_name ?? 'Procesor'}</h3>
							<p class="vendor">{cpuVendor}</p>
							<div class="facts">
								<span class="fact"
									>{statics?.physical_cores ?? '—'} fyzických / {statics?.logical_cores ?? '—'} logických
									jader</span
								>
								<span class="fact"
									>{hw?.cpu_thermal?.clock_mhz ?? '—'} MHz z {hw?.cpu_thermal?.max_mhz ?? '—'} MHz</span
								>
								<span class="fact"
									>L1 {statics?.l1_kb ?? '—'} kB · L2 {statics?.l2_kb ?? '—'} kB · L3 {statics?.l3_kb ??
										'—'} kB</span
								>
								{#if hw?.cpu_thermal?.celsius != null}
									<span class="fact"
										>{Math.round(hw.cpu_thermal.celsius)} °C · zdroj {hw.cpu_thermal.temp_source}</span
									>
								{:else}
									<span class="fact muted"
										>teplotu tenhle stroj z Windows nehlásí — ukázala by se, kdyby běžel HWiNFO nebo
										LibreHardwareMonitor</span
									>
								{/if}
							</div>
						</div>
						<div class="side">
							<span class="metric">{sys ? Math.round(sys.cpu_pct) : '—'}<small>%</small></span>
							{#if hw?.cpu_thermal?.throttling}
								<span class="pill warn">běží pod maximem</span>
							{:else}
								<span class="pill ok">jede naplno</span>
							{/if}
						</div>
					{:else if r.kind === 'ram'}
						<div class="ico"><MemoryStick size={17} /></div>
						<div class="info">
							<h3>Paměť</h3>
							<p class="vendor">{statics?.ram_modules?.[0]?.manufacturer ?? '—'}</p>
							<div class="facts">
								<span class="fact">{gb(sys?.mem_total_mb)} celkem</span>
								<span class="fact"
									>{statics?.ram_modules?.length ?? 0} modulů ve {statics?.ram_slots ?? '—'} slotech</span
								>
								{#each statics?.ram_modules ?? [] as m, i (m.slot + i)}
									<span class="fact muted">
										{m.slot}: {(m.size_mb / 1024).toFixed(0)} GB @ {m.configured_mts || '—'} MT/s
										(umí {m.speed_mts || '—'}) · {m.part_number || '—'}
									</span>
								{/each}
							</div>
						</div>
						<div class="side">
							<span class="metric">{ramPct ?? '—'}<small>%</small></span>
							<span class="pill dim">{gb(sys?.mem_used_mb)} z {gb(sys?.mem_total_mb)}</span>
						</div>
					{:else if r.kind === 'gpu'}
						{@const live = isLiveGpu(r.dev)}
						<div class="ico"><Microchip size={17} /></div>
						<div class="info">
							<h3>{r.dev.name}</h3>
							<p class="vendor">{r.dev.manufacturer || '—'}</p>
							<div class="facts">
								{#if driver(r.dev)}<span class="fact">{driver(r.dev)}</span>{/if}
								{#if live && sys?.gpu?.vram_used_mb != null}
									<span class="fact">VRAM {gb(sys.gpu.vram_used_mb)} z {gb(sys.gpu.vram_total_mb)}</span>
								{/if}
								{#if live && sys?.gpu?.clock_mhz != null}
									<span class="fact">{sys.gpu.clock_mhz} MHz</span>
								{/if}
								{#if live && sys?.gpu?.power_w != null}
									<span class="fact">{Math.round(sys.gpu.power_w)} W</span>
								{/if}
								{#if !live}
									<span class="fact muted">zatížení ani teplotu tahle karta přes ovladač nehlásí</span>
								{/if}
								{#if hwid(r.dev)}<span class="fact mono muted">{hwid(r.dev)}</span>{/if}
							</div>
						</div>
						<div class="side">
							{#if live}
								<span class="metric">{Math.round(sys.gpu_pct)}<small>%</small></span>
							{/if}
							{#if live && sys?.gpu?.temp_c != null}
								<span class="pill {tempClass(sys.gpu.temp_c, 75, 88)}"
									>{Math.round(sys.gpu.temp_c)} °C</span
								>
							{:else if r.dev.problem_code}
								<span class="pill bad">problém {r.dev.problem_code}</span>
							{:else}
								<span class="pill quiet">v pořádku</span>
							{/if}
						</div>
					{:else if r.kind === 'disk'}
						<div class="ico"><HardDrive size={17} /></div>
						<div class="info">
							<h3>{r.disk.model || `Disk ${r.disk.index}`}</h3>
							<p class="vendor">{diskVendor(r.disk.model)}</p>
							<div class="facts">
								{#each volumesOf(r.disk.index) as v (v.letter)}
									<span class="fact">
										{v.letter}: {v.label || v.fs} — {bytes(v.free_bytes)} volných z {bytes(
											v.total_bytes
										)}
									</span>
								{/each}
								{#if r.disk.power_on_hours != null}
									<span class="fact muted">{hours(r.disk.power_on_hours)}</span>
								{/if}
								{#if r.disk.spare_pct != null}
									<span class="fact muted">rezervní bloky {r.disk.spare_pct} %</span>
								{/if}
								{#if r.disk.temp_c == null && r.disk.used_pct == null}
									<span class="fact muted">zdraví přes SMART umí NVMe disky; tenhle ho nedává</span>
								{/if}
							</div>
						</div>
						<div class="side">
							{#if r.disk.temp_c != null}
								<span class="metric small {tempClass(r.disk.temp_c, 55, 70)}"
									>{r.disk.temp_c}<small>°C</small></span
								>
							{/if}
							{#if r.disk.critical}
								<span class="pill bad">SMART hlásí problém</span>
							{:else if r.disk.used_pct != null}
								<span class="pill {r.disk.used_pct >= 80 ? 'warn' : 'ok'}"
									>opotřebení {r.disk.used_pct} %</span
								>
							{:else}
								<span class="pill dim">zdraví nehlásí</span>
							{/if}
						</div>
					{:else if r.kind === 'board'}
						<div class="ico"><CircuitBoard size={17} /></div>
						<div class="info">
							<h3>{hw.board.product || 'Základní deska'}</h3>
							<p class="vendor">{hw.board.manufacturer || '—'}</p>
							<div class="facts">
								{#if hw.board.version}<span class="fact">revize {hw.board.version}</span>{/if}
								<span class="fact"
									>BIOS {hw.board.bios_version || '—'} · {hw.board.bios_date || '—'}</span
								>
								<span class="fact muted">{hw.board.bios_vendor}</span>
								{#if hw.board.system_product}
									<span class="fact muted"
										>stroj: {hw.board.system_manufacturer} {hw.board.system_product}</span
									>
								{/if}
							</div>
						</div>
						<div class="side"><span class="pill quiet">v pořádku</span></div>
					{:else if r.kind === 'battery'}
						<div class="ico"><BatteryCharging size={17} /></div>
						<div class="info">
							<h3>Baterie</h3>
							<p class="vendor">
								{#if hw.battery.charging}nabíjí se{:else if hw.battery.ac_online}napájení ze sítě{:else}běží
									z baterie{/if}
							</p>
							<div class="facts">
								{#if hw.battery.wear_pct != null}
									<span class="fact">
										nabije se na {(hw.battery.full_mwh / 1000).toFixed(1)} Wh z původních
										{(hw.battery.design_mwh / 1000).toFixed(1)} Wh
									</span>
								{/if}
								{#if hw.battery.cycles != null}
									<span class="fact muted">{hw.battery.cycles} nabíjecích cyklů</span>
								{/if}
							</div>
						</div>
						<div class="side">
							<span class="metric">{hw.battery.percent ?? '—'}<small>%</small></span>
							{#if hw.battery.wear_pct != null}
								<span class="pill {hw.battery.wear_pct >= 30 ? 'warn' : 'ok'}">
									opotřebení {Math.round(hw.battery.wear_pct)} %
								</span>
							{:else}
								<span class="pill dim">kapacity nehlásí</span>
							{/if}
						</div>
					{/if}
				</article>
			{/each}
		{/if}

		{#if visibleDisplays.length}
			<h2 class="sect" id="sect-Obrazovky"><Monitor size={14} /> Obrazovky</h2>
			{#each visibleDisplays as d, i (d.adapter + i)}
				<article class="item">
					<div class="ico"><Monitor size={17} /></div>
					<div class="info">
						<h3>{d.monitor || 'Obrazovka'}</h3>
						<p class="vendor">{d.adapter}</p>
						<div class="facts">
							<span class="fact">{d.width} × {d.height} bodů</span>
							<span class="fact">{d.refresh_hz} Hz</span>
						</div>
					</div>
					<div class="side">
						{#if d.primary}<span class="pill dim">hlavní</span>{/if}
						<span class="pill ok">připojená</span>
					</div>
				</article>
			{/each}
		{/if}

		{#each sections.filter((s) => s.name !== 'Komponenty' && s.name !== 'Obrazovky') as s (s.name)}
			<h2 class="sect" id="sect-{s.name}">
				<s.icon size={14} />
				{s.name}
				<span class="sect-n">{s.count}</span>
			</h2>
			{#each deviceSections.get(s.name) ?? [] as d, i (d.name + d.hardware_id + i)}
				{@const Ico = iconOf(d)}
				{@const rid = `dev-${s.name}-${i}`}
				<article class="item" id={rid} class:flash={flashId === rid} class:bad={d.problem_code}>
					<div class="ico"><Ico size={17} /></div>
					<div class="info">
						<h3>{d.name}</h3>
						<p class="vendor">{d.manufacturer || '—'}</p>
						<div class="facts">
							{#if driver(d)}<span class="fact">{driver(d)}</span>{/if}
							{#if d.class_desc}<span class="fact muted">{d.class_desc}</span>{/if}
							{#if hwid(d)}<span class="fact mono muted">{hwid(d)}</span>{/if}
						</div>
					</div>
					<div class="side">
						{#if d.problem_code}
							<span class="pill bad"><TriangleAlert size={11} /> problém {d.problem_code}</span>
						{:else}
							<span class="pill quiet">v pořádku</span>
						{/if}
					</div>
				</article>
			{/each}
		{/each}

		{#if !sections.length}
			<p class="empty">
				{filter ? 'Nic neodpovídá hledání.' : 'Soupis hardwaru se načítá…'}
			</p>
		{/if}
	</div>
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
		gap: 12px;
	}

	/* ── Pevná hlavička ── */
	.head {
		display: flex;
		flex-direction: column;
		gap: 10px;
		flex: none;
	}
	.head-top {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.search {
		display: flex;
		align-items: center;
		gap: 8px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 7px 11px;
		color: var(--text-dim);
		flex: 1;
		max-width: 340px;
	}
	.search input {
		flex: 1;
		min-width: 0;
		background: none;
		border: none;
		outline: none;
		color: var(--text);
		font: inherit;
		font-size: 0.82rem;
	}
	.clear {
		background: none;
		border: none;
		color: var(--text-dim);
		cursor: pointer;
		font-size: 1rem;
		line-height: 1;
		padding: 0 2px;
	}
	.clear:hover {
		color: var(--text);
	}
	.total {
		font-size: 0.8rem;
		color: var(--text-dim);
		font-variant-numeric: tabular-nums;
	}
	.total strong {
		color: var(--text);
		font-weight: 600;
	}

	/* Skok na problém — opakovaným klikáním se mezi nimi cykluje. */
	.alarm {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		margin-left: auto;
		background: color-mix(in srgb, var(--danger) 12%, transparent);
		border: 1px solid color-mix(in srgb, var(--danger) 45%, transparent);
		border-radius: 999px;
		color: var(--danger);
		font: inherit;
		font-size: 0.78rem;
		padding: 6px 8px 6px 12px;
		cursor: pointer;
	}
	.alarm:hover {
		background: color-mix(in srgb, var(--danger) 20%, transparent);
	}
	.alarm-go {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		background: color-mix(in srgb, var(--danger) 22%, transparent);
		border-radius: 999px;
		padding: 2px 6px 2px 8px;
		font-variant-numeric: tabular-nums;
	}

	/* ── Přepínač kategorií ── */
	.cats {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		padding-bottom: 10px;
		border-bottom: 1px solid var(--border);
	}
	.cat {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		background: none;
		border: 1px solid var(--border);
		border-radius: 999px;
		color: var(--text-dim);
		font: inherit;
		font-size: 0.76rem;
		padding: 5px 11px;
		cursor: pointer;
		transition:
			color 0.12s ease,
			border-color 0.12s ease,
			background 0.12s ease;
	}
	.cat:hover {
		color: var(--text);
		border-color: var(--border-strong, var(--border));
	}
	.cat.on {
		color: var(--bg);
		background: var(--accent);
		border-color: var(--accent);
	}
	.cat-n {
		font-variant-numeric: tabular-nums;
		opacity: 0.65;
	}

	/* ── Tělo ── */
	.body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding-right: 6px;
	}

	/* Nadpis kategorie zůstává nalepený nahoře, dokud sekce scrolluje —
	   je pořád vidět, ve které části seznamu uživatel je. */
	.sect {
		position: sticky;
		top: 0;
		z-index: 1;
		display: flex;
		align-items: center;
		gap: 9px;
		margin: 20px 0 9px;
		padding: 7px 2px 8px;
		font-size: 0.82rem;
		font-weight: 600;
		letter-spacing: 0.02em;
		color: var(--text);
		background: linear-gradient(var(--bg) 78%, transparent);
	}
	.sect:first-child {
		margin-top: 0;
	}
	.sect::after {
		content: '';
		flex: 1;
		height: 1px;
		background: var(--border);
	}
	.sect-n {
		font-weight: 400;
		font-size: 0.74rem;
		color: var(--text-dim);
		font-variant-numeric: tabular-nums;
		background: var(--surface-hover);
		border-radius: 999px;
		padding: 1px 8px;
	}

	/* ── Karta zařízení ── */
	.item {
		display: grid;
		grid-template-columns: 34px minmax(0, 1fr) minmax(120px, auto);
		gap: 12px;
		align-items: start;
		padding: 11px 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		margin-bottom: 6px;
		background: var(--surface);
		scroll-margin: 16px;
	}
	.item:hover {
		background: var(--surface-hover);
	}
	.item.bad {
		border-color: color-mix(in srgb, var(--danger) 45%, var(--border));
	}
	/* Skok na problém ho na chvíli zvýrazní, ať je jasné, kam to skočilo. */
	.item.flash {
		animation: flash 2.2s ease-out;
	}
	@keyframes flash {
		0%,
		55% {
			border-color: var(--danger);
			background: color-mix(in srgb, var(--danger) 14%, transparent);
		}
		100% {
			border-color: var(--border);
			background: var(--surface);
		}
	}

	.ico {
		display: grid;
		place-items: center;
		width: 34px;
		height: 34px;
		border-radius: 9px;
		background: var(--surface-hover);
		color: var(--text-dim);
	}
	.item.bad .ico {
		color: var(--danger);
		background: color-mix(in srgb, var(--danger) 14%, transparent);
	}

	.info {
		min-width: 0;
	}
	/* Název je výrazně větší než zbytek — je to hlavní informace. */
	.info h3 {
		margin: 0;
		font-size: 0.95rem;
		font-weight: 600;
		line-height: 1.3;
		word-break: break-word;
	}
	.vendor {
		margin: 1px 0 0;
		font-size: 0.76rem;
		color: var(--text-dim);
	}
	.facts {
		display: flex;
		flex-wrap: wrap;
		gap: 5px;
		margin-top: 7px;
	}
	.fact {
		font-size: 0.73rem;
		line-height: 1.35;
		padding: 2px 8px;
		border-radius: 6px;
		background: var(--surface-hover);
		color: var(--text);
	}
	.fact.muted {
		background: none;
		padding-left: 0;
		color: var(--text-dim);
	}
	.fact.mono {
		font-family: var(--font-mono);
		font-size: 0.68rem;
	}

	.side {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 5px;
		text-align: right;
	}
	/* Vytížení je jen číslo — grafy má Tasks, tohle není správce úloh. */
	.metric {
		font-size: 1.35rem;
		font-weight: 600;
		line-height: 1;
		font-variant-numeric: tabular-nums;
	}
	.metric.small {
		font-size: 1.1rem;
	}
	.metric small {
		font-size: 0.7rem;
		font-weight: 400;
		color: var(--text-dim);
		margin-left: 2px;
	}
	.pill {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		font-size: 0.73rem;
		padding: 3px 9px;
		border-radius: 999px;
		border: 1px solid transparent;
		white-space: nowrap;
	}
	.pill.ok {
		color: var(--ok);
		background: color-mix(in srgb, var(--ok) 12%, transparent);
	}
	.pill.warn {
		color: var(--warn);
		background: color-mix(in srgb, var(--warn) 14%, transparent);
	}
	.pill.bad {
		color: var(--danger);
		background: color-mix(in srgb, var(--danger) 14%, transparent);
	}
	.pill.dim {
		color: var(--text-dim);
		background: var(--surface-hover);
	}
	/* Zdravé zařízení se hlásí tiše: čitelný text a malá tečka.
	   Kdyby u 190 položek svítila zelená, byla by z barvy dekorace —
	   a červené na jedné rozbité položce by si nikdo nevšiml. */
	.pill.quiet {
		color: var(--text-dim);
		background: none;
		padding-left: 0;
		padding-right: 0;
	}
	.pill.quiet::before {
		content: '';
		width: 5px;
		height: 5px;
		border-radius: 50%;
		background: var(--ok);
		opacity: 0.55;
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
	.pill.cool {
		background: color-mix(in srgb, var(--ok) 12%, transparent);
	}
	.pill.warm {
		background: color-mix(in srgb, var(--warn) 14%, transparent);
	}
	.pill.hot {
		background: color-mix(in srgb, var(--danger) 14%, transparent);
	}

	.empty {
		color: var(--text-dim);
		font-size: 0.84rem;
		padding: 20px 0;
	}
</style>
