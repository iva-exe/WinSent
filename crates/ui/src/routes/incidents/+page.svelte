<script>
	// Incidenty (v3, SPEC kap. 16): záseky, pády aplikací a BSOD pod
	// jedním modelem. Seznam + detail s křivkou okna T-5min..T+30s.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import {
		TriangleAlert,
		Zap,
		MonitorX,
		Timer,
		RefreshCw,
		Trash2,
		FileText,
		EyeOff,
		Download,
		Check,
		ChevronRight
	} from 'lucide-svelte';
	import Sparkline from '$lib/Sparkline.svelte';

	let incidents = $state([]);
	let selected = $state(null);
	let windowPoints = $state([]);
	let loadError = $state('');

	// Druh incidentu → vizuál.
	const kinds = {
		stall: { label: 'Zásek systému', icon: Timer, color: 'var(--warn)' },
		// Pád aplikace je nepříjemnost, BSOD je porucha celého stroje —
		// barvy to musí odlišit, jinak vypadá všechno stejně vážně.
		app_crash: { label: 'Pád aplikace', icon: Zap, color: 'var(--warn)' },
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

	// Vybraná položka časové osy (ne jen incident — může to být i samotné
	// hlášení Windows, ke kterému žádný náš incident není).
	let selRow = $state(null);

	async function selectRow(row) {
		selRow = row;
		selected = row.incident;
		windowPoints = [];
		// Okno metrik: u našeho incidentu ho známe, u hlášení Windows
		// se odvodí od času pádu. Když v tu dobu hlídač neběžel, prostě
		// nic nepřijde a UI to řekne — nedomýšlí se.
		const base = row.ts;
		const from = row.incident?.window_from ?? base - 300;
		const to = row.incident?.window_to ?? base + 30;
		try {
			windowPoints = await invoke('query_system_history', { from, to });
		} catch {
			windowPoints = [];
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

	// Jeden seznam ze dvou zdrojů.
	//
	// Naše incidenty (co hlídač viděl na vlastní oči) a hlášení, která
	// si uložily Windows, jsou dva pohledy na tutéž věc. Náš pád
	// aplikace a hlášení Windows o témže pádu je JEDNA událost —
	// spáruje se podle času a jména, ať uživatel nevidí dva řádky
	// o jednom pádu. V detailu je pak vidět, ze kterých zdrojů to je.
	//
	// Co se nespáruje, zůstane samostatně: Windows zaznamenají pád
	// i tehdy, když hlídač neběžel, a hlídač vidí záseky, o kterých
	// Windows nevědí.
	const PAIR_WINDOW_S = 120;

	let timeline = $derived.by(() => {
		const used = new Set();
		const rows = incidents.map((i) => {
			const d = parseDetail(i);
			// Jméno, pod kterým bychom pád našli v hlášení Windows.
			const name = (i.culprit ?? d.name ?? '').toLowerCase();
			let report = null;
			if (i.kind === 'app_crash' && name) {
				for (let k = 0; k < crashes.length; k++) {
					if (used.has(k)) continue;
					const c = crashes[k];
					if (
						Math.abs(c.ts - i.ts) <= PAIR_WINDOW_S &&
						(c.app.toLowerCase() === name || name.includes(c.app.toLowerCase()))
					) {
						report = c;
						used.add(k);
						break;
					}
				}
			}
			return { key: `i${i.id}`, ts: i.ts, incident: i, report };
		});
		crashes.forEach((c, k) => {
			if (used.has(k)) return;
			rows.push({ key: `c${k}:${c.ts}`, ts: c.ts, incident: null, report: c });
		});
		return rows.filter((r) => !hidden.has(r.key)).sort((a, b) => b.ts - a.ts);
	});

	// Index bodu nejblíž okamžiku incidentu (marker ve sparkline).
	let markerIdx = $derived.by(() => {
		if (!selRow || windowPoints.length === 0) return null;
		let best = 0;
		for (let k = 1; k < windowPoints.length; k++) {
			if (Math.abs(windowPoints[k].ts - selRow.ts) < Math.abs(windowPoints[best].ts - selRow.ts))
				best = k;
		}
		return best;
	});

	// Skrytí řádku, který pochází JEN z protokolu Windows.
	//
	// Smazat se nedá — je to jejich záznam, ne náš, a sahat do protokolu
	// událostí by bylo přesně to, co tenhle nástroj nedělá. Schová se
	// tedy jen z našeho seznamu a pamatuje se to mezi spuštěními.
	const HIDDEN_KEY = 'winsent.hiddenCrashes';
	let hidden = $state(new Set());

	function loadHidden() {
		try {
			hidden = new Set(JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? '[]'));
		} catch {
			hidden = new Set();
		}
	}

	function hideReport(row, ev) {
		ev?.stopPropagation();
		const s = new Set(hidden);
		s.add(row.key);
		hidden = s;
		try {
			localStorage.setItem(HIDDEN_KEY, JSON.stringify([...s]));
		} catch {
			/* bez úložiště to platí aspoň do zavření okna */
		}
		if (selRow?.key === row.key) {
			selRow = null;
			selected = null;
		}
	}

	// Hlášení o pádech, která mají uložená Windows.
	//
	// Nezávislé na našich incidentech: Windows zaznamenávají pád i tehdy,
	// když jsme zrovna neběželi, a vědí u něj to podstatné — ve kterém
	// modulu to spadlo. Bez toho je „aplikace spadla" k ničemu.
	let crashes = $state([]);

	async function loadCrashes() {
		try {
			crashes = await invoke('query_crash_reports', { limit: 40 });
		} catch {
			crashes = [];
		}
	}

	// Podklady, které se dotahují až při exportu.
	//
	// Do souboru má jít všechno, na co umíme přijít — tabulka procesů
	// v okamžiku pádu, hodnoty jednotlivých jader, disků a GPU, soupis
	// hardwaru i ovladačů. V UI by to byla zeď, v souboru pro odborníka
	// nebo model je to přesně to, oč jde. Načítá se proto až na kliknutí,
	// ne dopředu.
	async function gatherExtras(row) {
		const out = { procs: [], detail: null, hw: null, drivers: null, sys: null };
		try {
			const r = await invoke('query_procs_at', { ts: row.ts });
			out.procs = r?.rows ?? [];
			out.procsTs = r?.ts;
		} catch {
			/* v historii už nic není */
		}
		try {
			out.detail = await invoke('query_detail_at', { ts: row.ts });
		} catch {
			/* jádra/disky/GPU z té doby nemáme */
		}
		try {
			out.hw = await invoke('query_hardware');
		} catch {
			/* hardware je aktuální stav, ne stav v čase pádu */
		}
		try {
			out.drivers = await invoke('query_drivers');
		} catch {
			/* bez ovladačů to bude jen kratší */
		}
		try {
			out.sys = await invoke('query_sys_info');
		} catch {
			/* nevadí */
		}
		return out;
	}

	// Export incidentu do textového souboru.
	//
	// Účel je konkrétní: poslat to někomu, kdo tomu rozumí — člověku
	// nebo modelu. Proto se do souboru dává VŠECHNO, co k incidentu
	// máme, i to, co je v UI schované, a v podobě, která se dá přečíst
	// bez téhle aplikace. Žádné odesílání nikam: soubor se uloží
	// a co s ním bude dál, rozhoduje uživatel.
	function reportText(row, x) {
		const i = row.incident;
		const rep = row.report;
		const d = i ? parseDetail(i) : {};
		const L = [];
		L.push('WINSENT — ZÁZNAM O INCIDENTU');
		L.push('='.repeat(60));
		L.push(`Kdy:      ${fmtTs(row.ts)}  (unix ${row.ts})`);
		L.push(`Druh:     ${i ? kindOf(i.kind).label : 'Pád aplikace'}`);
		L.push(
			`Zdroje:   ${[i ? 'hlídač Winsent' : null, rep ? 'protokol Windows' : null]
				.filter(Boolean)
				.join(' + ')}`
		);
		L.push('');

		if (rep) {
			L.push('CO O TOM PÍŠOU WINDOWS');
			L.push('-'.repeat(60));
			L.push(rep.summary);
			L.push('');
			L.push(rep.detail);
			if (rep.repeats > 1) L.push(`Opakování: ${rep.repeats}x ve stejném místě`);
			L.push('');
		}

		if (i) {
			L.push('CO VIDĚL HLÍDAČ');
			L.push('-'.repeat(60));
			L.push(`Viník:      ${i.culprit ?? 'nezjištěn'}`);
			if (i.identity_key) L.push(`Identita:   ${i.identity_key}`);
			if (i.kind === 'stall') {
				L.push(`Výpadek:    ${d.lag_ms ?? '—'} ms`);
				L.push(`Příčina:    ${causes[d.cause] ?? d.cause ?? '—'}`);
			}
			if (i.kind === 'app_crash' && d.exit_code != null) {
				L.push(
					`Exit kód:   0x${d.exit_code.toString(16).toUpperCase().padStart(8, '0')} (${d.exit_code})`
				);
				L.push(`Proces:     ${d.name || '—'}`);
			}
			if (i.kind === 'bsod') {
				L.push(
					`Bugcheck:   ${d.bugcheck != null ? '0x' + d.bugcheck.toString(16).toUpperCase().padStart(8, '0') : '—'}`
				);
				if (d.human) L.push(`Význam:     ${d.human}`);
				if (d.params) L.push(`Parametry:  ${JSON.stringify(d.params)}`);
				if (d.dump) L.push(`Minidump:   ${d.dump}`);
			}
			if (i.etl_path) L.push(`Černá skříňka: ${i.etl_path}`);
			L.push(`Okno:       ${fmtTs(i.window_from ?? row.ts - 300)} .. ${fmtTs(i.window_to ?? row.ts + 30)}`);
			if (d.top?.length) {
				L.push('');
				L.push('Nejnáročnější procesy v okně:');
				for (const t of d.top) L.push(`  ${String(t.pid).padStart(6)}  ${t.name || '(bez jména)'}  ${t.value}`);
			}
			L.push('');
			L.push('Surový detail (JSON):');
			L.push(i.detail ?? '{}');
			L.push('');
		}

		L.push('CO DĚLAL POČÍTAČ V TU CHVÍLI');
		L.push('-'.repeat(60));
		if (windowPoints.length > 1) {
			L.push('čas                     CPU%   RAM MB   síť dolů B/s   síť nahoru B/s');
			for (const p of windowPoints) {
				L.push(
					[
						fmtTs(p.ts).padEnd(22),
						String(Math.round(p.cpu_pct)).padStart(5),
						String(p.mem_used_mb).padStart(8),
						String(p.net_rx_bps).padStart(14),
						String(p.net_tx_bps).padStart(15)
					].join(' ')
				);
			}
		} else {
			L.push('Z té doby nejsou naměřená data — hlídač tehdy neběžel,');
			L.push('nebo jsou vzorky za hranicí retence.');
		}
		if (rep?.raw) {
			L.push('');
			L.push('SYROVÝ ZÁZNAM Z PROTOKOLU WINDOWS');
			L.push('-'.repeat(60));
			L.push(rep.raw);
			L.push('');
		}

		if (x?.detail) {
			L.push('');
			L.push('KOMPONENTY V OKAMŽIKU INCIDENTU');
			L.push('-'.repeat(60));
			L.push(`(nejbližší uložený vzorek: ${fmtTs(x.detail.ts)})`);
			if (x.detail.cores?.length) {
				L.push('');
				L.push('Zátěž jader:');
				x.detail.cores.forEach((c, k) => L.push(`  jádro ${String(k).padStart(3)}  ${c.toFixed(1)} %`));
			}
			if (x.detail.disks?.length) {
				L.push('');
				L.push('Disky (B/s):');
				L.push('  disk        čtení          zápis');
				for (const d of x.detail.disks) {
					L.push(
						`  ${String(d.index).padStart(4)}  ${String(d.r_bps).padStart(12)}  ${String(d.w_bps).padStart(13)}`
					);
				}
			}
			if (x.detail.gpu) {
				const g = x.detail.gpu;
				L.push('');
				L.push('GPU:');
				L.push(`  teplota:     ${g.temp_c ?? '—'} °C`);
				L.push(`  VRAM:        ${g.vram_used_mb ?? '—'} / ${g.vram_total_mb ?? '—'} MB`);
				L.push(`  spotřeba:    ${g.power_w ?? '—'} W`);
				L.push(`  takt:        ${g.clock_mhz ?? '—'} MHz`);
			}
		}

		if (x?.procs?.length) {
			L.push('');
			L.push('VŠECHNY PROCESY V OKAMŽIKU INCIDENTU');
			L.push('-'.repeat(60));
			L.push(`(vzorek z ${fmtTs(x.procsTs ?? row.ts)}, ${x.procs.length} procesů)`);
			L.push('   PID  název                          CPU%      RAM MB   čtení B/s  zápis B/s');
			for (const p of x.procs) {
				L.push(
					[
						String(p.pid).padStart(6),
						'  ',
						(p.name ?? '').padEnd(30).slice(0, 30),
						String((p.cpu_pct ?? 0).toFixed(1)).padStart(5),
						String(Math.round((p.ws_bytes ?? 0) / 1048576)).padStart(12),
						String(p.disk_r_bps ?? 0).padStart(12),
						String(p.disk_w_bps ?? 0).padStart(11)
					].join('')
				);
			}
		}

		if (x?.sys) {
			L.push('');
			L.push('SESTAVA POČÍTAČE');
			L.push('-'.repeat(60));
			const s = x.sys;
			L.push(`CPU:  ${s.cpu_name ?? '—'}  (${s.physical_cores ?? '?'} jader / ${s.logical_cores ?? '?'} vláken, ${s.cpu_base_mhz ?? '?'} MHz)`);
			L.push(`GPU:  ${s.gpu_name ?? '—'}`);
			for (const m of s.ram_modules ?? []) {
				L.push(`RAM:  ${m.size_mb} MB @ ${m.speed_mts ?? '?'} MT/s  slot ${m.slot ?? '?'}  ${m.manufacturer ?? ''} ${m.part_number ?? ''}`);
			}
			for (const d of s.disks ?? []) L.push(`Disk: [${d.index}] ${d.model}`);
		}

		if (x?.hw) {
			const h = x.hw;
			L.push('');
			L.push('HARDWARE — AKTUÁLNÍ STAV');
			L.push('-'.repeat(60));
			L.push('(soupis je z doby exportu, ne z okamžiku incidentu)');
			if (h.board) {
				L.push(`Deska: ${h.board.manufacturer ?? ''} ${h.board.product ?? ''}  BIOS ${h.board.bios_version ?? '?'} z ${h.board.bios_date ?? '?'}`);
			}
			if (h.cpu_thermal) {
				L.push(
					`Teplota CPU: ${h.cpu_thermal.celsius ?? '—'} °C (zdroj: ${h.cpu_thermal.temp_source}), takt ${h.cpu_thermal.clock_mhz ?? '?'}/${h.cpu_thermal.max_mhz ?? '?'} MHz, omezení: ${h.cpu_thermal.throttling ? 'ano' : 'ne'}`
				);
			}
			for (const d of h.disks ?? []) {
				L.push(
					`Disk [${d.index}] ${d.model}: teplota ${d.temp_c ?? '—'} °C, opotřebení ${d.used_pct ?? '—'} %, rezerva ${d.spare_pct ?? '—'} %, kritických ${d.critical ?? '—'}`
				);
			}
			for (const v of h.volumes ?? []) {
				L.push(
					`Svazek ${v.letter}: ${v.label || '(bez názvu)'} ${v.fs}  volno ${Math.round(v.free_bytes / 1e9)} / ${Math.round(v.total_bytes / 1e9)} GB`
				);
			}
			if (h.battery) {
				L.push(`Baterie: ${h.battery.percent ?? '—'} %, opotřebení ${h.battery.wear_pct ?? '—'} %`);
			}
			L.push('');
			L.push(`Zařízení (${(h.devices ?? []).length}):`);
			for (const d of h.devices ?? []) {
				L.push(
					`  ${(d.group_name || d.name || '').padEnd(42).slice(0, 42)} ${(d.manufacturer ?? '').padEnd(24).slice(0, 24)} ${d.driver_version ?? ''} ${d.driver_date ?? ''}${d.problem_code ? `  PROBLÉM ${d.problem_code}` : ''}`
				);
				L.push(`      ${d.class} · ${d.hardware_id}`);
			}
		}

		if (x?.drivers?.drivers?.length) {
			L.push('');
			L.push(`OVLADAČE (${x.drivers.drivers.length})`);
			L.push('-'.repeat(60));
			for (const d of x.drivers.drivers) {
				L.push(
					`  ${(d.device ?? '').padEnd(40).slice(0, 40)} ${(d.provider ?? '').padEnd(26).slice(0, 26)} ${(d.version ?? '').padEnd(18)} ${d.date ?? ''}${d.third_party ? '  [od výrobce]' : ''}${d.problem_code ? `  PROBLÉM ${d.problem_code}` : ''}`
				);
			}
		}

		L.push('');
		L.push('-'.repeat(60));
		L.push('Vygeneroval Winsent. Údaje pocházejí z tohoto počítače;');
		L.push('nic se nikam neodesílá — co se souborem bude dál, je na tobě.');
		return L.join('\n');
	}

	// Stav tlačítka: sbírám → hotovo. Bez odezvy uživatel neví, jestli
	// se kliknutí vůbec chytlo — a sběr podkladů chvíli trvá.
	let exportState = $state('idle');
	let exportName = $state('');

	async function downloadReport(row) {
		if (exportState === 'busy') return;
		exportState = 'busy';
		try {
			const extras = await gatherExtras(row);
			const stamp = new Date(row.ts * 1000)
				.toISOString()
				.slice(0, 19)
				.replace(/[:T]/g, '-');
			const name = `winsent-incident-${stamp}.txt`;
			const blob = new Blob([reportText(row, extras)], {
				type: 'text/plain;charset=utf-8'
			});
			const a = document.createElement('a');
			a.href = URL.createObjectURL(blob);
			a.download = name;
			a.click();
			setTimeout(() => URL.revokeObjectURL(a.href), 5000);
			exportName = name;
			exportState = 'done';
			setTimeout(() => {
				if (exportState === 'done') exportState = 'idle';
			}, 6000);
		} catch (e) {
			exportName = String(e);
			exportState = 'error';
			setTimeout(() => {
				if (exportState === 'error') exportState = 'idle';
			}, 6000);
		}
	}

	onMount(() => {
		loadHidden();
		load();
		loadCrashes();
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
	{:else if timeline.length === 0}
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
				{#each timeline as row (row.key)}
					{@const i = row.incident}
					{@const k = i ? kindOf(i.kind) : kinds.app_crash}
					<li>
						<button class="row" class:active={selRow?.key === row.key} onclick={() => selectRow(row)}>
							<span class="kind-ico" style:color={k.color}><k.icon size={16} /></span>
							<span class="row-main">
								<span class="row-title">
									{k.label}
									<!-- Odkud to víme. Řádek bez našeho incidentu je
									     pád, který zaznamenaly jen Windows — typicky
									     proto, že hlídač tehdy neběžel. -->
									{#if !i}
										<span class="src">jen ze záznamu Windows</span>
									{:else if row.report}
										<span class="src both">hlídač + Windows</span>
									{/if}
								</span>
								<span class="row-culprit">
									{row.report?.app ?? i?.culprit ?? '—'}
								</span>
							</span>
							<span class="row-ts">{fmtTs(row.ts)}</span>
							{#if i}
								<span
									class="row-del"
									role="button"
									tabindex="-1"
									title="Odstranit záznam (v systému se nic nemění)"
									onclick={(ev) => remove(i, ev)}
									onkeydown={() => {}}><Trash2 size={15} /></span
								>
							{:else}
								<span
									class="row-del"
									role="button"
									tabindex="-1"
									title="Skrýt ze seznamu (protokol Windows zůstane nedotčený)"
									onclick={(ev) => hideReport(row, ev)}
									onkeydown={() => {}}><EyeOff size={15} /></span
								>
							{/if}
						</button>
					</li>
				{/each}
			</ul>

			<section class="detail">
				{#if !selRow}
					<p class="empty">Vyber položku vlevo.</p>
				{:else}
					{@const selected = selRow.incident}
					{@const rep = selRow.report}
					{@const k = selected ? kindOf(selected.kind) : kinds.app_crash}
					{@const d = selected ? parseDetail(selected) : {}}
					<div class="d-head">
						<span class="kind-ico big" style:color={k.color}><k.icon size={21} /></span>
						<div>
							<h2>{k.label}</h2>
							<span class="d-ts">{fmtTs(selRow.ts)}</span>
						</div>
					</div>

					<!-- Co o tom píšou Windows. Nahoře, protože tohle je ta
					     věta, kvůli které sem člověk přišel. -->
					{#if rep}
						<p class="story">{rep.summary}</p>
						<pre class="story-detail">{rep.detail}</pre>
						{#if rep.repeats > 1}
							<p class="story-rep">Totéž se stalo {rep.repeats}× ve stejném místě.</p>
						{/if}
					{/if}

					{#if selected}
					<div class="d-grid">
						<div class="d-item wide">
							<span class="d-label">Viník</span>
							<span class="d-value">{selected.culprit ?? "nezjištěn"}</span>
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
					{/if}

					<h3 class="sec">Co dělal počítač v tu chvíli</h3>
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
						<!-- Poctivě: metriky máme jen po dobu retence a hlášení
						     Windows bývají starší. Nedomýšlí se nic. -->
						<p class="empty small">
							Z té doby už nemáme naměřená data — hlídač tehdy neběžel,
							nebo jsou vzorky za hranicí retence.
						</p>
					{/if}
					<!-- Celý incident do textového souboru. Účel je poslat to
					     někomu, kdo tomu rozumí — člověku nebo modelu. Proto
					     tam jde všechno, i to, co je v UI schované. -->
					<button
						class="export"
						class:done={exportState === 'done'}
						class:err={exportState === 'error'}
						disabled={exportState === 'busy'}
						onclick={() => downloadReport(selRow)}
					>
						{#if exportState === 'busy'}
							<RefreshCw size={15} class="spin" />
							Sbírám podklady…
						{:else if exportState === 'done'}
							<Check size={15} />
							Uloženo: {exportName}
						{:else if exportState === 'error'}
							<TriangleAlert size={15} />
							Nepovedlo se: {exportName}
						{:else}
							<Download size={15} />
							Stáhnout vše jako text
						{/if}
					</button>
					<p class="foot">
						Záznam jde odstranit ikonou koše v seznamu — maže se jen tenhle zápis,
						v systému se nic nemění.
					</p>
				{/if}
			</section>
		</div>
	{/if}
	<p class="page-note">
		Seznam spojuje dva zdroje: co viděl hlídač na vlastní oči a co si o pádu uložily samy
		Windows. Pád, který zachytily oba, je jeden řádek — u ostatních je napsáno, odkud
		informace je. Modul, ve kterém pád nastal, říká, KDE se to stalo, ne proč; když je to
		systémová knihovna, obvykle to neznamená chybu Windows.
	</p>
</div>

<style>
	/* Export — akce, ne dekorace, takže výrazněji než poznámka pod ní. */
	.export {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		margin-top: 16px;
		padding: 7px 13px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--surface);
		color: var(--text-dim);
		font: inherit;
		font-size: 0.82rem;
		cursor: pointer;
	}
	.export:disabled {
		opacity: 0.75;
		cursor: default;
	}
	/* Potvrzení, že se soubor opravdu uložil — a jak se jmenuje. */
	.export.done {
		color: var(--ok);
		border-color: color-mix(in srgb, var(--ok) 55%, transparent);
		background: color-mix(in srgb, var(--ok) 10%, transparent);
	}
	.export.err {
		color: var(--danger);
		border-color: color-mix(in srgb, var(--danger) 55%, transparent);
	}
	:global(.export .spin) {
		animation: spin 1s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
	.export:hover {
		color: var(--text);
		background: var(--surface-hover);
		box-shadow: inset 0 0 0 1px var(--border-strong);
	}
	/* Odkud informace je — drobné, ať nepřebije název incidentu. */
	.src {
		font-family: var(--font-mono);
		font-size: 0.62rem;
		letter-spacing: 0.02em;
		padding: 1px 6px;
		border-radius: 999px;
		border: 1px solid var(--border);
		color: var(--text-faint);
		vertical-align: middle;
	}
	.src.both {
		border-color: color-mix(in srgb, var(--ok) 45%, transparent);
		color: var(--ok);
	}
	/* Věta, kvůli které sem člověk přišel — nahoře a čitelně. */
	.story {
		margin: 14px 0 0;
		font-size: 0.98rem;
		line-height: 1.5;
	}
	.story-detail {
		margin: 8px 0 0;
		padding: 10px 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		font-family: var(--font-mono);
		font-size: 0.72rem;
		line-height: 1.6;
		color: var(--text-dim);
		white-space: pre-wrap;
		word-break: break-word;
	}
	.story-rep {
		margin: 8px 0 0;
		font-size: 0.8rem;
		color: var(--warn);
	}
	.page-note {
		margin: 14px 0 0;
		font-size: 0.78rem;
		line-height: 1.55;
		color: var(--text-faint);
	}
	.note {
		margin: 12px 0 0;
		font-size: 0.78rem;
		line-height: 1.55;
		color: var(--text-faint);
	}
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
