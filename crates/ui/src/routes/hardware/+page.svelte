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
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { openMenu, akceKopirovat, oddelovac } from '$lib/itemmenu.svelte.js';
	import { describeProblem } from '$lib/devproblem.js';
	import { CATEGORIES, categoryOf, hasVidPid } from '$lib/devcategory.js';
	import { mergeSame } from '$lib/mergesame.js';
	import CategoryNav from '$lib/CategoryNav.svelte';
	import {
		AudioLines,
		BatteryCharging,
		Bluetooth,
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
	// Předvyplněné hledání z jiné sekce (pravý klik → Zobrazit zařízení).
	let filter = $state(page.url.searchParams.get('q') ?? '');

	let bodyEl = $state(null);
	// Přepínač kategorií i jeho scroll žijí ve sdílené komponentě —
	// Drivers mají tentýž kus a dvě kopie by se v chování rozešly.
	let catNav = $state(null);
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

	// Kategorie jsou sdílené s Ovladači ($lib/devcategory.js) — ovladač
	// nese tytéž třídy jako zařízení, kterému patří, takže „Periferie"
	// znamená na obou obrazovkách totéž. Dvě kopie téhle tabulky by se
	// při první přidané třídě rozešly.

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
		const cat = categoryOf(dev, hasVidPid(dev.hardware_id));
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

	// ── Kontextové menu ──
	//
	// Co vyhledat je tady nejdůležitější rozhodnutí celé nabídky.
	// Nadpis karty je často obecný („NVMe", „Paměť RAM") a model, na
	// který se uživatel ptá, leží až v popiscích pod ním. `openMenu`
	// dostane kandidáty v pořadí od nejobecnějšího a sám vybere ten
	// první konkrétní; když je nadpis obecný, spojí ho s modelem —
	// z „NVMe" + „ST2000DM008-2FR102" vyjde dotaz na obojí.
	function menuKomponenta(e, r) {
		const spolecne = (title, hledat, subtitle = '', extra = []) =>
			openMenu(e, {
				title,
				subtitle,
				hledat,
				items: [...extra, oddelovac, akceKopirovat(hledat.filter(Boolean)[0] ?? title)]
			});

		if (r.kind === 'cpu') {
			const jmeno = statics?.cpu_name ?? 'Procesor';
			return spolecne('Procesor', [jmeno, cpuVendor], jmeno);
		}
		if (r.kind === 'ram') {
			const m = statics?.ram_modules?.[0];
			// U paměti hledá člověk konkrétní modul, ne slovo „RAM“.
			const model = [m?.manufacturer, m?.part_number].filter(Boolean).join(' ');
			return spolecne('Paměť RAM', [model || 'RAM', m?.part_number], model);
		}
		if (r.kind === 'gpu') {
			return spolecne(r.dev.name, [r.dev.name, r.dev.manufacturer], r.dev.manufacturer, [
				{
					label: 'Ovladač zařízení',
					icon: 'info',
					hint: r.dev.driver_version ?? '',
					disabled: !r.dev.driver_version,
					run: () => goto(`/drivers?q=${encodeURIComponent(r.dev.name)}`)
				}
			]);
		}
		if (r.kind === 'disk') {
			// Nadpis bývá model, ale když ne, spojí se s ním typ disku.
			return spolecne(r.disk.model || `Disk ${r.disk.index}`, [r.disk.model, 'disk'], '', [
				{
					label: 'Ukázat ve Files',
					icon: 'disk',
					run: () => goto('/files')
				}
			]);
		}
		if (r.kind === 'board') {
			const b = hw?.board;
			const model = [b?.manufacturer, b?.product].filter(Boolean).join(' ');
			return spolecne('Základní deska', [model || 'základní deska', b?.product], model, [
				{
					label: 'Hledat aktualizaci BIOSu',
					icon: 'search',
					hint: b?.bios_version ?? '',
					disabled: !model,
					run: () => invoke('search_web', { query: `${model} BIOS update` })
				}
			]);
		}
		if (r.kind === 'battery') {
			return spolecne('Baterie', ['baterie notebooku opotřebení'], '');
		}
		return spolecne(r.id, [r.id]);
	}

	function menuObrazovka(e, d) {
		openMenu(e, {
			title: d.monitor || 'Obrazovka',
			subtitle: d.adapter ?? '',
			// Jméno monitoru je konkrétní; adaptér je grafika, ne obrazovka.
			hledat: [d.monitor, d.adapter],
			items: [oddelovac, akceKopirovat(d.monitor || d.adapter)]
		});
	}

	function menuZarizeni(e, d, mg) {
		const problem = mg.members.find((m) => m.problem_code);
		openMenu(e, {
			title: d.name,
			subtitle: vendorSaysNothing(d.manufacturer) ? (hwid(d) ?? '') : d.manufacturer,
			// Výrobce sám o sobě nestačí, ale se jménem zařízení dá
			// smysluplný dotaz; hardwarové ID je poslední záchrana.
			hledat: [d.name, d.manufacturer, hwid(d)],
			items: [
				problem
					? {
							label: 'Co znamená ten problém?',
							icon: 'search',
							hint: `kód ${problem.problem_code}`,
							run: () =>
								invoke('search_web', {
									query: `Windows Device Manager error code ${problem.problem_code}`
								})
						}
					: null,
				{
					label: 'Ovladač zařízení',
					icon: 'info',
					hint: d.driver_version ?? '',
					disabled: !d.driver_version,
					run: () => goto(`/drivers?q=${encodeURIComponent(d.name)}`)
				},
				oddelovac,
				akceKopirovat(d.name),
				akceKopirovat(d.hardware_id, 'Kopírovat hardwarové ID')
			]
		});
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

	// Rozbalené skupiny zařízení (klíč skupiny).
	let openDevices = $state(new Set());
	function toggleDevice(key) {
		const s = new Set(openDevices);
		if (s.has(key)) s.delete(key);
		else s.add(key);
		openDevices = s;
	}

	// Výrobce, který nic neříká — Windows ho doplní, když se zařízení
	// nehlásí samo („(Standard system devices)", „Microsoft").
	function vendorSaysNothing(m) {
		return !m || m.startsWith('(') || m === 'Microsoft';
	}

	// Zařízení sloučená do skutečných kusů hardwaru a rozdělená do
	// kategorií. Procesor, grafika a disky se přeskočí — mají vlastní
	// bohatší karty nahoře.
	//
	// Windows rozepíšou jeden kus hardwaru na řadu zařízení: rozhraní,
	// HID kolekce, vlastní sběrnice výrobce. Naměřeno na tomhle stroji
	// 16 řádků pro jednu myš a 10 pro jeden bezdrátový přijímač. Co
	// k sobě patří, určuje group_key ze služby; tady se z toho skládá
	// jeden řádek se vším, co členové vědí.
	let deviceSections = $derived.by(() => {
		const map = new Map(CATEGORIES.map((c) => [c.name, []]));
		const groups = new Map();
		const rank = new Map(CATEGORIES.map((c, i) => [c.name, i]));

		for (const d of hw?.devices ?? []) {
			if (d.class === 'Processor' || d.class === 'DiskDrive' || d.class === 'Display') continue;
			const key = d.group_key || `${d.hardware_id}|${d.name}`;
			let g = groups.get(key);
			if (!g) {
				g = {
					key,
					name: d.group_name || d.name,
					manufacturer: '',
					class_desc: d.class_desc,
					hardware_id: d.hardware_id,
					driver_version: d.driver_version,
					driver_date: d.driver_date,
					problem_code: 0,
					category: categoryOf(d, hasVidPid(d.hardware_id)),
					icon: d,
					members: []
				};
				groups.set(key, g);
			}
			g.members.push(d);
			// Vykřičník u kteréhokoliv rozhraní je problém celého
			// zařízení — schovat ho do rozkliku by znamenalo zamlčet ho.
			if (d.problem_code && !g.problem_code) {
				g.problem_code = d.problem_code;
				g.class_desc = d.class_desc;
			}
			// Ze jmen výrobců vyhrává ten, který opravdu někoho jmenuje.
			if (vendorSaysNothing(g.manufacturer) && !vendorSaysNothing(d.manufacturer)) {
				g.manufacturer = d.manufacturer;
				g.icon = d;
			}
			// Skupina sedí v té nejvýstižnější kategorii svých členů:
			// bezdrátový přijímač klávesnice je periferie, ne řadič,
			// i když jedno jeho rozhraní je třídy USB.
			const cat = categoryOf(d, hasVidPid(d.hardware_id));
			if ((rank.get(cat) ?? 99) < (rank.get(g.category) ?? 99)) g.category = cat;
		}

		for (const g of groups.values()) {
			// Hledá se napříč celou skupinou — jinak by filtr našel
			// zařízení podle názvu, který je schovaný pod rozklikem.
			const hay = g.members
				.map((m) => `${m.name} ${m.manufacturer} ${m.class_desc} ${m.class}`)
				.join(' ');
			if (!matches(`${g.name} ${hay}`)) continue;
			map.get(g.category)?.push(g);
		}
		// Druhá úroveň: co se jmenuje stejně, do jednoho řádku.
		//
		// Po sloučení na fyzická zařízení zbývá sedm „PCI HOST Bridge",
		// osm „Volume" a šest „Motherboard resources" — pro uživatele
		// sedmkrát tentýž řádek. Skupina ale netvrdí, že je to jeden
		// kus: nese počet a pod rozklikem každý kus zvlášť.
		for (const [cat, list] of map) {
			map.set(
				cat,
				mergeSame(list, (g) => `${g.name}|${g.class_desc ?? ''}`)
			);
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

	// Kolik je kusů hardwaru dohromady (bez filtru počítá všechny
	// skupiny, i ty, které padly mimo zobrazené kategorie).
	let deviceCount = $derived(
		[...deviceSections.values()].reduce((n, list) => n + list.length, 0)
	);

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
			// Po sloučení duplicit sedí data v `head`, ne na obalu —
			// bez toho zůstal seznam problémů prázdný a tlačítko nahoře
			// se vůbec neukázalo.
			(deviceSections.get(s.name) ?? []).forEach((mg, i) => {
				const bad = mg.members.find((m) => m.problem_code) ?? mg.head;
				if (bad.problem_code) out.push({ id: `dev-${s.name}-${i}`, label: bad.name });
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
</script>

<div class="page">
	<!-- ── Pevná hlavička: nescrolluje pryč. Stejná stavba jako
	     ostatní sekce: h1 · údaje · filtr vpravo. ── -->
	<header class="head">
		<div class="head-top">
			<h1>Hardware</h1>
			<!-- Počítají se kusy hardwaru, ne řádky ze systému. Číslo
			     v závorce říká, kolik zařízení z toho Windows udělaly. -->
			<span class="total label-tech" title="V systému {hw?.devices?.length ?? 0} zařízení">
				{deviceCount} zařízení
				{#if (hw?.devices?.length ?? 0) > deviceCount}
					<i>({hw.devices.length} položek systému)</i>
				{/if}
			</span>
			{#if problems.length}
				<button class="alarm" onclick={jumpToProblem}>
					<TriangleAlert size={16} />
					{problems.length}
					{problems.length === 1 ? 'problém' : problems.length < 5 ? 'problémy' : 'problémů'}
					<span class="alarm-go">
						{problemIdx >= 0 ? `${problemIdx + 1}/${problems.length}` : 'ukázat'}
						<ChevronRight size={15} />
					</span>
				</button>
			{/if}
			<div class="filter">
				<Search size={16} />
				<input placeholder="hledat zařízení nebo výrobce…" bind:value={filter} />
				{#if filter}
					<button class="clear" onclick={() => (filter = '')}>×</button>
				{/if}
			</div>
		</div>
		<CategoryNav bind:this={catNav} {sections} {bodyEl} idPrefix="sect" />
	</header>

	<!-- ── Tělo: jediná scrollovaná oblast ── -->
	<div class="body" bind:this={bodyEl} onscroll={() => catNav?.onScroll()}>
		{#if loadError}
			<p class="empty">Nelze načíst hardware: {loadError}</p>
		{/if}

		{#if componentRows.length}
			<section class="grp" id="sect-Komponenty">
			<h2 class="sect"><Cpu size={17} /> Komponenty <span class="sect-n">{componentRows.length}</span></h2>
			{#each componentRows as r (r.id)}
				<article class="item" id={r.id} class:flash={flashId === r.id} class:bad={r.problem} oncontextmenu={(e) => menuKomponenta(e, r)}>
					{#if r.kind === 'cpu'}
						<div class="ico"><Cpu size={20} /></div>
						<div class="info">
							<h3>{statics?.cpu_name ?? 'Procesor'}</h3>
							<p class="vendor">{cpuVendor}</p>
							<dl class="facts">
								<div>
									<dt>Jádra</dt>
									<dd>{statics?.physical_cores ?? '—'} fyz. / {statics?.logical_cores ?? '—'} log.</dd>
								</div>
								<div>
									<dt>Takt</dt>
									<dd>
										{hw?.cpu_thermal?.clock_mhz ?? '—'} MHz
										<span class="note-v">z {hw?.cpu_thermal?.max_mhz ?? '—'} MHz</span>
									</dd>
								</div>
								{#if statics?.cpu_base_mhz}
									<div><dt>Základní takt</dt><dd>{statics.cpu_base_mhz} MHz</dd></div>
								{/if}
								<div>
									<dt>Mezipaměť</dt>
									<dd>
										{statics?.l1_kb ?? '—'} / {statics?.l2_kb ?? '—'} / {statics?.l3_kb ?? '—'} kB
										<span class="note-v">L1 / L2 / L3</span>
									</dd>
								</div>
								<!-- Teplota se nikdy nepředstírá (SPEC 15.2): buňka
								     zůstane, ale je v ní pomlčka a proč. Tady je
								     mřížka faktů, takže díra po vynechané buňce by
								     mátla víc než přiznaná pomlčka; v textovém
								     záznamu se řádek naopak vynechává. -->
								<div>
									<dt>Teplota</dt>
									{#if hw?.cpu_thermal?.celsius != null}
										<dd>
											{Math.round(hw.cpu_thermal.celsius)} °C
											<span class="note-v">zdroj: {hw.cpu_thermal.temp_source}</span>
										</dd>
									{:else}
										<dd class="dim">
											—
											<span class="note-v">
												Windows teplotu jádra nevydávají; přečte se, když běží HWiNFO
												se zapnutou sdílenou pamětí nebo LibreHardwareMonitor
											</span>
										</dd>
									{/if}
								</div>
							</dl>
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
						<div class="ico"><MemoryStick size={20} /></div>
						<div class="info">
							<h3>Paměť</h3>
							<p class="vendor">{statics?.ram_modules?.[0]?.manufacturer ?? '—'}</p>
							<dl class="facts">
								<div><dt>Celkem</dt><dd>{gb(sys?.mem_total_mb)}</dd></div>
								<div>
									<dt>Moduly</dt>
									<dd>
										{statics?.ram_modules?.length ?? 0} ze {statics?.ram_slots ?? '—'} slotů
									</dd>
								</div>
								<!-- Popiskem je slot — mono verzálky na „DIMM_A1"
								     sedí doslova a moduly se pod sebou zarovnají. -->
								{#each statics?.ram_modules ?? [] as m, i (i + ':' + m.slot)}
									<div>
										<dt>{m.slot || `Modul ${i + 1}`}</dt>
										<dd>
											{(m.size_mb / 1024).toFixed(0)} GB @ {m.configured_mts || '—'} MT/s
											<span class="note-v">
												umí {m.speed_mts || '—'}{m.part_number ? ` · ${m.part_number}` : ''}
											</span>
										</dd>
									</div>
								{/each}
							</dl>
						</div>
						<div class="side">
							<span class="metric">{ramPct ?? '—'}<small>%</small></span>
							<span class="pill dim">{gb(sys?.mem_used_mb)} z {gb(sys?.mem_total_mb)}</span>
						</div>
					{:else if r.kind === 'gpu'}
						{@const live = isLiveGpu(r.dev)}
						<div class="ico"><Microchip size={20} /></div>
						<div class="info">
							<h3>{r.dev.name}</h3>
							<p class="vendor">{r.dev.manufacturer || '—'}</p>
							<dl class="facts">
								{#if r.dev?.driver_version}
									<div><dt>Ovladač</dt><dd>{r.dev.driver_version}</dd></div>
								{/if}
								{#if r.dev?.driver_date}
									<div><dt>Datum</dt><dd>{r.dev.driver_date}</dd></div>
								{/if}
								<!-- Živé hodnoty jen u karty, kterou umíme číst.
								     Buňky se u ostatních vynechají celé — prázdná
								     buňka by naznačovala, že hodnota je nula. -->
								{#if live && sys?.gpu?.vram_used_mb != null}
									<div>
										<dt>VRAM</dt>
										<dd>
											{gb(sys.gpu.vram_used_mb)}
											<span class="note-v">z {gb(sys.gpu.vram_total_mb)}</span>
										</dd>
									</div>
								{/if}
								{#if live && sys?.gpu?.clock_mhz != null}
									<div><dt>Takt</dt><dd>{sys.gpu.clock_mhz} MHz</dd></div>
								{/if}
								{#if live && sys?.gpu?.power_w != null}
									<div><dt>Příkon</dt><dd>{Math.round(sys.gpu.power_w)} W</dd></div>
								{/if}
								{#if !live}
									<div class="wide">
										<dt>Telemetrie</dt>
										<dd class="dim">zatížení ani teplotu tahle karta přes ovladač nehlásí</dd>
									</div>
								{/if}
								{#if hwid(r.dev)}
									<div><dt>ID</dt><dd class="mono">{hwid(r.dev)}</dd></div>
								{/if}
							</dl>
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
						<div class="ico"><HardDrive size={20} /></div>
						<div class="info">
							<h3>{r.disk.model || `Disk ${r.disk.index}`}</h3>
							<p class="vendor">{diskVendor(r.disk.model)}</p>
							<dl class="facts">
								<!-- Popiskem svazku je jeho písmeno; hodnota je to,
								     co uživatele zajímá — kolik je volno. -->
								{#each volumesOf(r.disk.index) as v (v.letter)}
									<div>
										<dt>{v.letter}:</dt>
										<dd>
											{bytes(v.free_bytes)} volných
											<span class="note-v">
												z {bytes(v.total_bytes)}{v.label ? ` · ${v.label}` : ''} ({v.fs})
											</span>
										</dd>
									</div>
								{/each}
								{#if r.disk.power_on_hours != null}
									<div><dt>Provoz</dt><dd>{hours(r.disk.power_on_hours)}</dd></div>
								{/if}
								{#if r.disk.spare_pct != null}
									<div><dt>Rezerva</dt><dd>{r.disk.spare_pct} %</dd></div>
								{/if}
								{#if r.disk.used_pct != null}
									<div><dt>Opotřebení</dt><dd>{r.disk.used_pct} %</dd></div>
								{/if}
								{#if r.disk.temp_c == null && r.disk.used_pct == null}
									<div class="wide">
										<dt>Zdraví</dt>
										<dd class="dim">SMART přes NVMe umí jen novější disky; tenhle ho nedává</dd>
									</div>
								{/if}
							</dl>
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
						<div class="ico"><CircuitBoard size={20} /></div>
						<div class="info">
							<h3>{hw.board.product || 'Základní deska'}</h3>
							<p class="vendor">{hw.board.manufacturer || '—'}</p>
							<dl class="facts">
								{#if hw.board.version}
									<div><dt>Revize</dt><dd>{hw.board.version}</dd></div>
								{/if}
								<div>
									<dt>BIOS</dt>
									<dd>
										{hw.board.bios_version || '—'}
										<span class="note-v">{hw.board.bios_date || '—'}</span>
									</dd>
								</div>
								{#if hw.board.bios_vendor}
									<div><dt>BIOS od</dt><dd>{hw.board.bios_vendor}</dd></div>
								{/if}
								{#if hw.board.system_product}
									<div>
										<dt>Stroj</dt>
										<dd>{hw.board.system_manufacturer} {hw.board.system_product}</dd>
									</div>
								{/if}
							</dl>
						</div>
						<div class="side"><span class="pill quiet">v pořádku</span></div>
					{:else if r.kind === 'battery'}
						<div class="ico"><BatteryCharging size={20} /></div>
						<div class="info">
							<h3>Baterie</h3>
							<p class="vendor">
								{#if hw.battery.charging}nabíjí se{:else if hw.battery.ac_online}napájení ze sítě{:else}běží
									z baterie{/if}
							</p>
							<dl class="facts">
								{#if hw.battery.wear_pct != null}
									<div>
										<dt>Kapacita</dt>
										<dd>
											{(hw.battery.full_mwh / 1000).toFixed(1)} Wh
											<span class="note-v">
												z původních {(hw.battery.design_mwh / 1000).toFixed(1)} Wh
											</span>
										</dd>
									</div>
								{/if}
								{#if hw.battery.cycles != null}
									<div><dt>Cykly</dt><dd>{hw.battery.cycles}</dd></div>
								{/if}
								<!-- Zbývající čas dává smysl jen při běhu z baterie;
								     při napájení ze sítě je to nesmyslné číslo. -->
								{#if !hw.battery.ac_online && hw.battery.remaining_s}
									<div>
										<dt>Zbývá</dt>
										<dd>
											{Math.floor(hw.battery.remaining_s / 3600)} h
											{Math.round((hw.battery.remaining_s % 3600) / 60)} min
										</dd>
									</div>
								{/if}
							</dl>
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
			</section>
		{/if}

		{#if visibleDisplays.length}
			<section class="grp" id="sect-Obrazovky">
			<h2 class="sect"><Monitor size={17} /> Obrazovky <span class="sect-n">{visibleDisplays.length}</span></h2>
			{#each visibleDisplays as d, i (d.adapter + i)}
				<article class="item" oncontextmenu={(e) => menuObrazovka(e, d)}>
					<div class="ico"><Monitor size={20} /></div>
					<div class="info">
						<h3>{d.monitor || 'Obrazovka'}</h3>
						<dl class="facts">
							<div><dt>Rozlišení</dt><dd>{d.width} × {d.height}</dd></div>
							<div><dt>Obnovování</dt><dd>{d.refresh_hz} Hz</dd></div>
							<!-- Adaptér patří do mřížky s popiskem, ne na pozici
							     výrobce — výrobce monitoru to totiž není. -->
							<div><dt>Adaptér</dt><dd>{d.adapter}</dd></div>
						</dl>
					</div>
					<div class="side">
						{#if d.primary}<span class="pill dim">hlavní</span>{/if}
						<span class="pill ok">připojená</span>
					</div>
				</article>
			{/each}
			</section>
		{/if}

		{#each sections.filter((s) => s.name !== 'Komponenty' && s.name !== 'Obrazovky') as s (s.name)}
			<section class="grp" id="sect-{s.name}">
			<h2 class="sect">
				<s.icon size={17} />
				{s.name}
				<span class="sect-n">{s.count}</span>
			</h2>
			{#each deviceSections.get(s.name) ?? [] as mg, i (mg.key)}
				{@const d = mg.head}
				{@const Ico = iconOf(d.icon)}
				{@const rid = `dev-${s.name}-${i}`}
				{@const bad = mg.members.find((m) => m.problem_code)}
				{@const trouble = describeProblem(bad?.problem_code)}
				{@const open = openDevices.has(mg.key)}
				<!-- `mg.count > 1` = víc kusů se shodným jménem (sedm PCI
				     mostů). Řádek to nikdy nevydává za jeden kus — nese
				     počet a pod rozklikem je každý zvlášť. -->
				{@const parts = mg.count > 1 ? mg.members : d.members}
				<article class="item" id={rid} class:flash={flashId === rid} class:bad={mg.members.some((m) => m.problem_code)} oncontextmenu={(e) => menuZarizeni(e, d, mg)}>
					<div class="ico"><Ico size={20} /></div>
					<div class="info">
						<h3>
							{d.name}
							<!-- Dva různé důvody k rozkliku a nesmí se plést:
							     „kusů" = víc zařízení se shodným jménem,
							     „rozhraní" = jeden kus rozepsaný systémem. -->
							{#if mg.count > 1}
								<button class="parts many" onclick={() => toggleDevice(mg.key)}>
									{mg.count} kusů
									<ChevronRight class="parts-caret" size={12} strokeWidth={2.25} />
								</button>
							{:else if d.members.length > 1}
								<button class="parts" onclick={() => toggleDevice(mg.key)}>
									{d.members.length} rozhraní
									<ChevronRight class="parts-caret" size={12} strokeWidth={2.25} />
								</button>
							{/if}
						</h3>
						<p class="vendor">{d.manufacturer || '—'}</p>
						<!-- Mřížka popisek → hodnota. Dřív to byla řada štítků,
						     kde „ovladač 10.0.19041.1" a „Audio inputs and
						     outputs" vypadaly stejně a nešlo poznat, co je co.
						     Pořadí buněk je pevné napříč všemi kartami. -->
						<dl class="facts">
							{#if d.driver_version}
								<div><dt>Ovladač</dt><dd>{d.driver_version}</dd></div>
							{/if}
							{#if d.driver_date}
								<div><dt>Datum</dt><dd>{d.driver_date}</dd></div>
							{/if}
							{#if d.class_desc}
								<div><dt>Třída</dt><dd>{d.class_desc}</dd></div>
							{/if}
							{#if hwid(d)}
								<div><dt>ID</dt><dd class="mono">{hwid(d)}</dd></div>
							{/if}
						</dl>
						<!-- U rozbitého zařízení nestačí kód: musí být vidět,
						     co se děje a co to pro uživatele znamená. -->
						{#if trouble}
							<p class="trouble">
								<strong>{trouble.what}</strong>
								{trouble.means}
							</p>
						{/if}
						{#if open}
							<ul class="parts-list">
								{#each parts as m, mi (mi + ":" + (m.hardware_id ?? m.key))}
									<li class:bad={m.problem_code}>
										<span class="p-name">{m.name}</span>
										<span class="p-id mono">{m.hardware_id ?? (m.members?.[0]?.hardware_id ?? "")}</span>
										{#if m.problem_code}
											<span class="p-bad">problém {m.problem_code}</span>
										{/if}
									</li>
								{/each}
							</ul>
						{/if}
					</div>
					<div class="side">
						{#if bad}
							<span class="pill bad"><TriangleAlert size={14} /> problém {bad.problem_code}</span>
						{:else}
							<span class="pill quiet">v pořádku</span>
						{/if}
					</div>
				</article>
			{/each}
			</section>
		{/each}

			{#if !sections.length}
			<p class="empty">
				{filter ? 'Nic neodpovídá hledání.' : 'Soupis hardwaru se načítá…'}
			</p>
		{/if}
	</div>
</div>

<style>
	/* Doplněk pod hodnotou — kontext, který by v hodnotě dělal zmatek
	   („4200 MHz z 4700 MHz" se špatně čte, „4200 MHz" a pod tím
	   „z 4700 MHz" dobře). */
	.note-v {
		display: block;
		margin-top: 1px;
		font-size: var(--fs-xs);
		color: var(--text-faint);
		line-height: 1.35;
	}
	.facts dd.dim {
		color: var(--text-faint);
	}
	.facts .wide {
		grid-column: 1 / -1;
	}
	/* Mřížka popisek → hodnota.
	   Popisky jsou v jednom jazyce (mono verzálky) bez ohledu na to, že
	   hodnoty jsou různé typy — verze, datum, text, identifikátor.
	   Pořadí buněk je pevné napříč kartami, takže se dá očima skákat
	   po sloupci celým seznamem. */
	.facts {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(150px, max-content));
		gap: 14px 34px;
		margin: 12px 0 0;
	}
	.facts dt {
		font-family: var(--font-mono);
		font-size: var(--fs-3xs);
		letter-spacing: 0.05em;
		text-transform: uppercase;
		color: var(--text-faint);
	}
	.facts dd {
		margin: 2px 0 0;
		font-size: var(--fs-md);
		color: var(--text);
		line-height: 1.35;
		word-break: break-word;
	}
	.facts dd.mono {
		font-family: var(--font-mono);
		font-size: var(--fs-xs);
		color: var(--text-dim);
	}
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
	/* h1 a filtr stejné jako v ostatních sekcích (Programs, On start) —
	   jedna aplikace, jeden tvar hlavičky. */
	.head-top h1 {
		font-size: 1.2rem;
		font-weight: 600;
		margin: 0;
	}
	.filter {
		margin-left: auto;
		display: flex;
		align-items: center;
		gap: 6px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 5px 9px;
		color: var(--text-dim);
		background: var(--surface);
		width: 300px;
	}
	.filter input {
		flex: 1;
		min-width: 0;
		background: none;
		border: none;
		outline: none;
		color: var(--text);
		font: inherit;
		font-size: var(--fs-lg);
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
		font-variant-numeric: tabular-nums;
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
		font-size: var(--fs-lg);
		padding: 8px 10px 8px 15px;
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

	/* ── Tělo ── */
	.body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding-right: 6px;
	}

	/* Obal sekce. Bez něj se sticky nadpisy VŠECH minulých sekcí lepily
	   na horní hranu přes sebe — a skok na kategorii nahoru měřil pozici
	   přilepeného nadpisu místo začátku sekce, takže nikam neskočil.
	   Uvnitř obalu se nadpis lepí jen po dobu své sekce. */
	.grp {
		margin-top: 26px;
	}
	.grp:first-child {
		margin-top: 0;
	}

	/* Nadpis kategorie zůstává nalepený nahoře, dokud jeho sekce
	   scrolluje — je pořád vidět, ve které části seznamu uživatel je. */
	/* Typografie jako skupinové popisky v Programs a Files (label-tech):
	   mono, verzálky, prostrkané — jazyk celé aplikace. */
	.sect {
		position: sticky;
		top: 0;
		z-index: 1;
		display: flex;
		align-items: center;
		gap: 9px;
		margin: 0 0 11px;
		padding: 9px 2px 10px;
		font-family: var(--font-mono);
		font-size: var(--fs-md);
		font-weight: 500;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--text-dim);
		background: linear-gradient(var(--bg) 80%, transparent);
	}
	.sect::after {
		content: '';
		flex: 1;
		height: 1px;
		background: var(--border);
	}
	.sect-n {
		font-weight: 400;
		font-size: var(--fs-xs);
		color: var(--text-faint);
		font-variant-numeric: tabular-nums;
	}

	/* ── Karta zařízení ── */
	/* Pevná šířka stavového sloupce: stavy a čísla lícují pod sebou
	   napříč všemi kartami, ne podle délky textu v každé zvlášť. */
	.item {
		display: grid;
		grid-template-columns: 40px minmax(0, 1fr) 180px;
		gap: 14px;
		align-items: start;
		padding: 14px 16px;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		margin-bottom: 8px;
		background: var(--surface);
		scroll-margin: 20px;
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
		width: 40px;
		height: 40px;
		border-radius: 11px;
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
		font-size: 1.06rem;
		font-weight: 600;
		line-height: 1.3;
		word-break: break-word;
	}
	.vendor {
		margin: 3px 0 0;
		font-size: var(--fs-md);
		color: var(--text-dim);
	}
	/* Rozklik na jednotlivá rozhraní — drobný, ať nepřebije název. */
	.parts {
		display: inline-flex;
		align-items: center;
		gap: 3px;
		margin-left: 8px;
		padding: 1px 7px;
		border: 1px solid var(--border);
		border-radius: 999px;
		background: transparent;
		color: var(--text-faint);
		font-family: var(--font-mono);
		font-size: var(--fs-2xs);
		letter-spacing: 0.02em;
		cursor: pointer;
		vertical-align: middle;
	}
	/* Víc kusů se shodným jménem — odlišené, ať se to neplete
	   s rozhraními jednoho kusu. */
	.parts.many {
		border-color: var(--text-dim);
		color: var(--text-dim);
	}
	.parts:hover {
		color: var(--text);
		border-color: var(--text-dim);
	}
	.parts-list {
		margin: 8px 0 0;
		padding: 0 0 0 12px;
		list-style: none;
		border-left: 2px solid var(--border);
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.parts-list li {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 4px 10px;
		font-size: var(--fs-sm);
		color: var(--text-dim);
	}
	.parts-list li.bad .p-name {
		color: var(--danger);
	}
	.p-id {
		font-size: var(--fs-2xs);
		color: var(--text-faint);
		word-break: break-all;
	}
	.p-bad {
		font-size: var(--fs-2xs);
		color: var(--danger);
	}
	.fact {
		font-size: var(--fs-sm);
		line-height: 1.4;
		padding: 4px 11px;
		border-radius: 7px;
		background: var(--surface-hover);
		color: var(--text);
	}
	.fact.muted {
		background: none;
		padding-left: 2px;
		padding-right: 2px;
		color: var(--text-dim);
	}
	.fact.mono {
		font-family: var(--font-mono);
		font-size: var(--fs-xs);
	}

	/* Vysvětlení poruchy — co se děje a co to znamená. */
	.trouble {
		margin: 9px 0 0;
		font-size: var(--fs-sm);
		line-height: 1.45;
		color: var(--text-dim);
		border-left: 2px solid var(--danger);
		padding-left: 10px;
	}
	.trouble strong {
		display: block;
		color: var(--danger);
		font-weight: 600;
	}

	.side {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 7px;
		text-align: right;
	}
	/* Vytížení je jen číslo — grafy má Tasks, tohle není správce úloh. */
	.metric {
		font-size: 1.5rem;
		font-weight: 600;
		line-height: 1;
		font-variant-numeric: tabular-nums;
	}
	.metric.small {
		font-size: 1.25rem;
	}
	.metric small {
		font-size: var(--fs-sm);
		font-weight: 400;
		color: var(--text-dim);
		margin-left: 3px;
	}
	.pill {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		font-size: var(--fs-sm);
		padding: 4px 11px;
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
	/* Zdravé zařízení je čitelně označené, ale nekřičí: zelená tečka
	   a neutrální rámeček. Kdyby u 190 položek svítila plná zelená,
	   byla by z barvy dekorace — a červené na jedné rozbité položce
	   by si pak nikdo nevšiml. */
	.pill.quiet {
		color: var(--text-dim);
		background: var(--surface-hover);
		border-color: var(--border);
	}
	.pill.quiet::before {
		content: '';
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--ok);
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
		font-size: var(--fs-lg);
		padding: 20px 0;
	}
</style>
