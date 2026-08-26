// Záznam o celém počítači — textový soubor pro člověka i pro model.
//
// Stejný účel i tvar jako záznam o incidentu (routes/incidents): pošle
// se někomu, kdo tomu rozumí, a musí být čitelný bez téhle aplikace.
// Proto prostý text s popisky, ne JSON: model i člověk potřebují vědět,
// co které číslo znamená, a hlavičky sekcí jsou zároveň orientační body.
//
// CO SE DO ZÁZNAMU NEDÁVÁ: obsah disku. Žádné cesty k souborům, žádné
// seznamy složek, duplicit ani prázdných souborů, žádné mapy instalací.
// Z celé sekce Files jde do záznamu jen technika disků — model, zdraví,
// teplota, kapacita svazků. Uživatel si tenhle soubor někam pošle
// a v cestách typu `C:\Users\Jméno\Dokumenty\…` je víc o něm samotném
// než o stavu počítače.
//
// Odesílání odsud nikam nevede: soubor se uloží do Stažených souborů
// a co s ním bude dál, rozhoduje uživatel.

const SEP = '='.repeat(72);
const SUB = '-'.repeat(72);

// ── Skrývání identifikujících údajů ────────────────────────────────
//
// Záznam se posílá dál, takže z něj musí ven to, co identifikuje
// uživatele nebo jeho síť — ale tak, aby zbytek zůstal analyzovatelný.
// Proto se maskuje, nemaže: „192.168.x.x → 151.101.x.x, port 443,
// established" pořád říká, co se dělo, jen už ne komu to patří.
//
// Co se maskuje a proč:
//   · IP adresy — vlastní i protistrany. Vlastní veřejná IP je přímý
//     identifikátor, lokální prozrazuje topologii sítě.
//   · MAC — zůstává první polovina (výrobce karty, ta se hodí),
//     zbytek pryč: celá MAC je jedinečné číslo zařízení.
//   · SSID Wi-Fi — jméno sítě jde v databázích wardrivingu přeložit
//     na adresu domu. Tohle je z celého záznamu nejcitlivější údaj.
//   · SID účtu — část pro stroj pryč, RID zůstává (500 = Administrator,
//     1001 = první uživatel), protože ten nese význam.
//   · cesty do profilu — jméno složky uživatele nahrazeno.
//   · celá jména účtů — skutečné jméno člověka do záznamu nepatří.

const RE_IPV4 = /\b(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\b/g;
const RE_IPV6 = /\b(?:[0-9A-Fa-f]{1,4}:){2,7}[0-9A-Fa-f]{0,4}\b/g;
const RE_MAC = /\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b/g;
const RE_SID = /\bS-1-5-21-\d+-\d+-\d+-(\d+)\b/g;
// Oddělovač může být obojí. Registr obsahuje, co do něj instalátor
// zapsal, a Windows si poradí s `C:/Users/Jméno` stejně jako
// s `C:\Users\Jméno` — jenže maska hlídající jen zpětné lomítko takový
// řádek pustí dál. Naměřeno na položce po spuštění pro Movavi:
// `C:/Users/IVA/AppData/Roaming\Movavi…` prošla nemaskovaná, přestože
// všechny ostatní cesty v témž souboru maskované byly.
const RE_PROFILE = /([A-Za-z]:[\\/]Users[\\/])[^\\/:*?"<>|\s]+/gi;

/// Adresy, které nikoho neidentifikují — nechávají se celé.
const KEEP_IP = new Set(['0.0.0.0', '255.255.255.255', '::', '::1']);

function maskIp(v) {
	const s = String(v ?? '').trim();
	if (!s) return s;
	if (KEEP_IP.has(s) || s.startsWith('127.')) return s;
	if (s.includes(':')) {
		// IPv6: první dvě skupiny (rozsah poskytovatele) a zbytek pryč.
		const parts = s.split(':');
		return `${parts[0]}:${parts[1] ?? ''}:…`;
	}
	const o = s.split('.');
	return o.length === 4 ? `${o[0]}.${o[1]}.x.x` : s;
}

function maskMac(v) {
	const s = String(v ?? '').trim();
	if (!s) return s;
	const p = s.split(/[:-]/);
	return p.length === 6 ? `${p[0]}:${p[1]}:${p[2]}:xx:xx:xx` : 'xx:xx:xx:xx:xx:xx';
}

/// Volný text (příkazy, hlášení o pádech, cíle zásahů). Prochází
/// všemi vzory naráz — do těchhle řetězců nevidíme dopředu, takže se
/// spoléhat na strukturu nedá.
function clean(v) {
	let s = String(v ?? '');
	if (!s) return s;
	s = s.replace(RE_MAC, (m) => maskMac(m));
	s = s.replace(RE_IPV6, (m) => (m.includes('::') && m.length < 4 ? m : maskIp(m)));
	s = s.replace(RE_IPV4, (m) => maskIp(m));
	s = s.replace(RE_SID, (_m, rid) => `S-1-5-21-<stroj>-${rid}`);
	s = s.replace(RE_PROFILE, '$1<uživatel>');
	return s;
}

function maskList(a) {
	return (a ?? []).map(maskIp).join(', ');
}

function ts(t) {
	if (!t) return '—';
	const d = new Date(t * 1000);
	return `${d.toLocaleDateString('cs-CZ')} ${d.toLocaleTimeString('cs-CZ')}`;
}

/// Datum bez času, ISO. Ve zkráceném sloupci vždycky přežije rok —
/// „21. 2. 2026" useknuté na deset znaků dávalo „21. 2. 202", takže
/// sloupec s datem instalace neměl žádnou informační hodnotu.
function day(t) {
	if (!t) return '';
	const d = new Date(t * 1000);
	const p = (n) => String(n).padStart(2, '0');
	return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/// Doba lidsky. Sloupec „30 dnů" míchal „43200 m" a „65 min" — minuty
/// se u velkých čísel do sedmi znaků nevešly a text se ořízl uprostřed
/// jednotky. Nad hodinu a půl se proto přepíná na hodiny, nad dva dny
/// na dny.
function dur(secs) {
	if (secs == null) return '—';
	const m = Math.round(secs / 60);
	if (m < 90) return `${m} min`;
	const h = Math.round(m / 6) / 10;
	return h < 48 ? `${h} h` : `${Math.round(h / 24)} d`;
}

function gb(b) {
	return b == null ? '—' : `${(b / 1e9).toFixed(1)} GB`;
}

function pad(s, n) {
	return String(s ?? '').padEnd(n).slice(0, n);
}

// U služeb se „zapnuto" četlo jako „běží". Typ spuštění a aktuální stav
// jsou přitom dvě nezávislé věci, takže mají i dvě různá slova.
function startLabel(x) {
	if (x.source !== 'service') return x.enabled ? 'zapnuto' : 'vypnuto';
	return x.enabled ? 'auto' : 'ručně';
}

function runLabel(x) {
	if (x.running == null) return '—';
	return x.running ? 'běží' : 'stojí';
}

function num(v, n, dec = 0) {
	return (v == null ? '—' : Number(v).toFixed(dec)).padStart(n);
}

/// Posbírá všechno, co jde. Každý dotaz zvlášť v try — když jeden
/// kolektor mlčí (starší služba, chybějící čidlo), zbytek záznamu
/// tím nemá trpět a v souboru se to napíše.
export async function gatherAll(invoke, onStep = () => {}) {
	const now = Math.floor(Date.now() / 1000);
	const from = now - 24 * 3600;
	const out = { now, from, errors: [] };

	const grab = async (key, label, fn) => {
		onStep(label);
		try {
			out[key] = await fn();
		} catch (e) {
			out[key] = null;
			out.errors.push(`${label}: ${e}`);
		}
	};

	await grab('ping', 'služba', () => invoke('ping_daemon'));
	await grab('sysInfo', 'sestava počítače', () => invoke('query_sys_info'));
	await grab('system', 'aktuální metriky', () => invoke('query_system'));
	await grab('selfUsage', 'spotřeba Winsentu', () => invoke('query_self_usage'));
	await grab('health', 'zdraví sběračů', () => invoke('query_collector_health'));
	await grab('hw', 'hardware', () => invoke('query_hardware'));
	await grab('displays', 'obrazovky', () => invoke('query_displays'));
	await grab('drivers', 'ovladače', () => invoke('query_drivers'));
	await grab('security', 'ochrana a oprávnění', () => invoke('query_security'));
	await grab('permTotals', 'čas u oprávnění', () => invoke('query_perm_use_totals', { days: 30 }));
	await grab('users', 'účty', () => invoke('query_users'));
	await grab('conn', 'připojení', () => invoke('query_connection'));
	await grab('net', 'síť podle aplikací', () => invoke('query_network'));
	await grab('startup', 'po spuštění', () => invoke('query_startup'));
	await grab('apps', 'programy', () => invoke('query_apps'));
	await grab('procs', 'procesy', () => invoke('query_procs'));
	// Z Files jen disky — obsah se do záznamu nedává.
	await grab('volumes', 'disky a svazky', () => invoke('query_volumes'));
	await grab('incidents', 'incidenty', () => invoke('query_incidents', { limit: 500 }));
	await grab('crashes', 'hlášení Windows', () => invoke('query_crash_reports', { limit: 200 }));
	await grab('audit', 'historie zásahů', () => invoke('query_audit', { limit: 500 }));
	await grab('events', 'události za 24 h', () => invoke('query_events', { from, to: now }));
	await grab('sysHist', 'průběh systému za 24 h', () =>
		invoke('query_system_history', { from, to: now })
	);
	await grab('diskHist', 'zatížení disků za 24 h', () =>
		invoke('query_disk_history', { from, to: now })
	);
	return out;
}

export function reportText(d) {
	const L = [];
	const H = (t) => {
		L.push('');
		L.push(SEP);
		L.push(t);
		L.push(SEP);
	};
	const S = (t) => {
		L.push('');
		L.push(t);
		L.push(SUB);
	};

	// ── Hlavička ──
	L.push('WINSENT — ZÁZNAM O STAVU POČÍTAČE');
	L.push(SEP);
	L.push(`Vytvořeno:  ${ts(d.now)}  (unix ${d.now})`);
	// Nadpis „posledních 24 hodin" lhal, když tolik dat neexistuje —
	// čerstvě nainstalovaný nástroj má hodinu, po restartu systému se
	// jede od bootu. Píše se proto skutečně pokryté okno i důvod.
	const oldest = d.sysHist?.length ? d.sysHist[0].ts : null;
	const covered = oldest ? d.now - oldest : 0;
	const boot = d.system?.uptime_s != null ? d.now - d.system.uptime_s : null;
	L.push(`Okno dat:   ${ts(d.from)} .. ${ts(d.now)}  (žádáno 24 hodin)`);
	if (oldest) {
		const why =
			boot != null && oldest <= boot + 120
				? ''
				: boot != null && oldest > boot
					? '  — omezeno startem systému nebo instalací nástroje'
					: '';
		L.push(
			`Skutečně:   ${ts(oldest)} .. ${ts(d.now)}  (${dur(covered)}, ${d.sysHist.length} vzorků)${why}`
		);
	} else {
		L.push('Skutečně:   žádná naměřená historie — služba běží krátce');
	}
	if (boot != null) {
		L.push(`Systém běží od: ${ts(boot)}  (${dur(d.system.uptime_s)})`);
	}
	if (d.ping) {
		// Uptime služby a stáří dat jsou dvě různé věci: data přežijí
		// restart služby, protože bydlí v databázi.
		L.push(
			`Služba:     běží, protokol v${d.ping.protocol_version}, uptime ${dur(d.ping.uptime_s)}` +
				(d.ping.uptime_s < covered ? '  (data jsou starší — přežila restart služby)' : '')
		);
	} else {
		L.push('Služba:     NEBĚŽÍ — živé hodnoty v tomhle záznamu chybí');
	}
	L.push('');
	L.push('CO V ZÁZNAMU ZÁMĚRNĚ NENÍ');
	L.push(SUB);
	L.push('Obsah disku: žádné cesty k souborům, seznamy složek, duplicity');
	L.push('ani mapy instalací. Z disků jde do záznamu jen technika —');
	L.push('model, zdraví, teplota, kapacita.');
	L.push('');
	L.push('Identifikující údaje jsou maskované, ne smazané, aby zbytek šel');
	L.push('pořád analyzovat:');
	L.push('  · IP adresy    192.168.1.20 → 192.168.x.x   (vlastní i protistrany)');
	L.push('  · MAC adresa   zůstává jen výrobce karty, zbytek xx:xx:xx');
	L.push('  · Wi-Fi        jména sítí vynechána úplně — v databázích');
	L.push('                 wardrivingu se dají přeložit na adresu domu');
	L.push('  · SID účtu     část pro stroj pryč, RID zůstává (nese význam)');
	L.push('  · cesty        C:\\Users\\Jméno → C:\\Users\\<uživatel>');
	L.push('  · celá jména   skutečné jméno člověka u účtu vynecháno');
	L.push('');
	L.push('V záznamu naopak ZŮSTÁVAJÍ: přihlašovací jména účtů (o tom je celá');
	L.push('sekce ÚČTY), jména nainstalovaných programů a jména serverů, se');
	L.push('kterými aplikace mluví — bez nich by síťová část nedávala smysl.');
	if (d.errors?.length) {
		L.push('');
		L.push('Části, které se nepodařilo přečíst:');
		for (const e of d.errors) L.push(`  · ${e}`);
	}

	// ── Sestava ──
	if (d.sysInfo) {
		const s = d.sysInfo;
		H('SESTAVA POČÍTAČE');
		L.push(
			`CPU:   ${s.cpu_name ?? '—'}  (${s.physical_cores ?? '?'} jader / ${s.logical_cores ?? '?'} vláken, základní takt ${s.cpu_base_mhz ?? '?'} MHz)`
		);
		L.push(`GPU:   ${s.gpu_name ?? '—'}`);
		L.push(`RAM:   ${s.ram_modules?.length ?? 0} modulů ze ${s.ram_slots ?? '?'} slotů`);
		for (const m of s.ram_modules ?? []) {
			L.push(
				`       ${m.size_mb} MB @ ${m.configured_mts ?? '?'} MT/s (umí ${m.speed_mts ?? '?'})  slot ${m.slot ?? '?'}  ${m.manufacturer ?? ''} ${m.part_number ?? ''}`
			);
		}
		for (const k of s.disks ?? []) L.push(`Disk:  [${k.index}] ${k.model}`);
	}

	// ── Aktuální metriky ──
	if (d.system) {
		const s = d.system;
		H('AKTUÁLNÍ ZATÍŽENÍ');
		L.push(`CPU:        ${(s.cpu_pct ?? 0).toFixed(1)} %`);
		L.push(
			`RAM:        ${s.mem_used_mb} / ${s.mem_total_mb} MB (${(((s.mem_used_mb ?? 0) / (s.mem_total_mb || 1)) * 100).toFixed(1)} %)`
		);
		L.push(`GPU:        ${s.gpu_pct != null ? s.gpu_pct.toFixed(1) + ' %' : '—'}`);
		if (s.gpu) {
			L.push(
				`GPU detail: teplota ${s.gpu.temp_c ?? '—'} °C, VRAM ${s.gpu.vram_used_mb ?? '—'}/${s.gpu.vram_total_mb ?? '—'} MB, příkon ${s.gpu.power_w != null ? s.gpu.power_w.toFixed(1) : '—'} W, takt ${s.gpu.clock_mhz ?? '—'} MHz`
			);
		}
		L.push(`Síť:        ↓ ${s.net_rx_bps ?? 0} B/s   ↑ ${s.net_tx_bps ?? 0} B/s`);
		L.push(
			`Procesů:    ${s.proc_count ?? '—'}   vláken ${s.threads_total ?? '—'}   handlů ${s.handles_total ?? '—'}`
		);
		L.push(`Takt CPU:   ${s.cpu_clock_mhz ?? '—'} / ${s.cpu_clock_max_mhz ?? '—'} MHz`);
		L.push(`Uptime:     ${s.uptime_s ?? '—'} s`);
		if (s.cores?.length) {
			L.push('');
			L.push('Zátěž jader (%):');
			const per = 8;
			for (let i = 0; i < s.cores.length; i += per) {
				L.push(
					'  ' +
						s.cores
							.slice(i, i + per)
							.map((c, k) => `${String(i + k).padStart(3)}:${num(c, 6, 1)}`)
							.join('  ')
				);
			}
		}
		if (s.disks?.length) {
			L.push('');
			L.push('Disky teď (B/s):    čtení        zápis');
			for (const k of s.disks) {
				L.push(`  disk ${String(k.index).padStart(2)}  ${num(k.r_bps, 12)}  ${num(k.w_bps, 12)}`);
			}
		}
	}

	// ── Hardware ──
	if (d.hw) {
		const h = d.hw;
		H('HARDWARE');
		if (h.board) {
			L.push(
				`Deska:  ${h.board.manufacturer ?? ''} ${h.board.product ?? ''} ${h.board.version ?? ''}`
			);
			L.push(
				`BIOS:   ${h.board.bios_version ?? '?'} z ${h.board.bios_date ?? '?'} (${h.board.bios_vendor ?? '?'})`
			);
			if (h.board.system_product) {
				L.push(`Stroj:  ${h.board.system_manufacturer ?? ''} ${h.board.system_product}`);
			}
		}
		if (h.cpu_thermal) {
			const c = h.cpu_thermal;
			L.push(
				`CPU:    teplota ${c.celsius ?? '—'} °C (zdroj ${c.temp_source}), takt ${c.clock_mhz ?? '?'} / ${c.max_mhz ?? '?'} MHz, omezení: ${c.throttling ? 'ANO' : 'ne'}`
			);
		}
		if (h.battery) {
			const b = h.battery;
			L.push(
				`Baterie: ${b.percent ?? '—'} %, ${b.charging ? 'nabíjí se' : b.ac_online ? 'ze sítě' : 'z baterie'}, opotřebení ${b.wear_pct != null ? Math.round(b.wear_pct) + ' %' : '—'}, kapacita ${b.full_mwh ?? '—'}/${b.design_mwh ?? '—'} mWh, cyklů ${b.cycles ?? '—'}`
			);
		}
		S('Fyzické disky (SMART)');
		L.push('  # model                                    tepl.  opotř.  rezerva  provoz h  kritické');
		for (const k of h.disks ?? []) {
			L.push(
				`  ${String(k.index).padStart(1)} ${pad(k.model, 40)} ${num(k.temp_c, 5)}  ${num(k.used_pct, 5)}  ${num(k.spare_pct, 7)}  ${num(k.power_on_hours, 8)}  ${k.critical ? 'ANO' : 'ne'}`
			);
		}
		S('Svazky');
		L.push('  písm. název                fs      volno / celkem');
		for (const v of h.volumes ?? []) {
			L.push(
				`  ${pad(v.letter + ':', 6)} ${pad(v.label || '(bez názvu)', 20)} ${pad(v.fs, 7)} ${gb(v.free_bytes)} / ${gb(v.total_bytes)}`
			);
		}
		const bad = (h.devices ?? []).filter((x) => x.problem_code);
		S(`Zařízení (${(h.devices ?? []).length}, z toho s problémem ${bad.length})`);
		for (const x of bad) {
			L.push(`  PROBLÉM ${x.problem_code}  ${x.name}  [${x.class}]  ${x.hardware_id ?? ''}`);
		}
		L.push('');
		for (const x of h.devices ?? []) {
			L.push(
				`  ${pad(x.group_name || x.name, 44)} ${pad(x.manufacturer, 24)} ${pad(x.driver_version, 18)} ${x.driver_date ?? ''}`
			);
			L.push(`      ${x.class} · ${x.hardware_id ?? ''}`);
		}
	}

	if (d.displays?.length) {
		S('Obrazovky');
		for (const x of d.displays) {
			L.push(
				`  ${pad(x.monitor || '(bez názvu)', 30)} ${x.width}x${x.height} @ ${x.refresh_hz} Hz  ${x.primary ? '[hlavní] ' : ''}${x.adapter}`
			);
		}
	}

	// ── Disky z Files (JEN technika, žádný obsah) ──
	if (d.volumes) {
		H('DISKY — KAPACITA A ZDRAVÍ');
		L.push('(ze sekce Files se do záznamu bere jen tohle; obsah disku ne)');
		L.push('');
		for (const v of d.volumes.volumes ?? []) {
			const used = v.total_bytes ? ((v.total_bytes - v.free_bytes) / v.total_bytes) * 100 : 0;
			L.push(
				`  ${pad(v.letter + ':', 5)} ${pad(v.label || '(bez názvu)', 22)} ${pad(v.fs, 7)} obsazeno ${used.toFixed(1)} %  volno ${gb(v.free_bytes)} z ${gb(v.total_bytes)}  fyzický disk ${v.disk_index ?? '—'}`
			);
		}
		L.push('');
		for (const k of d.volumes.health ?? []) {
			L.push(
				`  disk ${k.index}: ${k.model} — teplota ${k.temp_c ?? '—'} °C, opotřebení ${k.used_pct ?? '—'} %, rezerva ${k.spare_pct ?? '—'} %, provoz ${k.power_on_hours ?? '—'} h${k.critical ? '  KRITICKÉ' : ''}`
			);
		}
	}

	// ── Ovladače ──
	if (d.drivers?.drivers?.length) {
		const list = d.drivers.drivers;
		H(`OVLADAČE (${list.length})`);
		L.push('  zařízení                                 dodavatel                  verze              datum');
		for (const x of list) {
			L.push(
				`  ${pad(x.device, 40)} ${pad(x.provider, 26)} ${pad(x.version, 18)} ${pad(x.date, 12)}${x.third_party ? ' [od výrobce]' : ''}${x.problem_code ? `  PROBLÉM ${x.problem_code}` : ''}`
			);
		}
	}

	// ── Bezpečnost ──
	if (d.security) {
		const p = d.security.protection ?? {};
		H('OCHRANA SYSTÉMU');
		for (const a of p.av ?? []) {
			const [name, enabled, fresh, leftover] = a;
			L.push(
				`Antivirus:  ${name} — ${leftover ? 'ZBYTEK PO ODINSTALACI (program na disku není)' : enabled ? 'běží' : 'vypnutý'}${leftover ? '' : `, definice ${fresh ? 'aktuální' : 'ZASTARALÉ'}`}`
			);
		}
		if (p.defender) {
			L.push(
				`Defender:   realtime ${p.defender[0] ? 'ano' : 'ne'}, definice staré ${p.defender[1] ?? '—'} d, rychlý sken před ${p.defender[2] ?? '—'} d`
			);
		}
		L.push(
			`Firewall:   doména ${p.fw_domain == null ? '—' : p.fw_domain ? 'ano' : 'NE'}, privátní ${p.fw_private == null ? '—' : p.fw_private ? 'ano' : 'NE'}, veřejná ${p.fw_public == null ? '—' : p.fw_public ? 'ano' : 'NE'}`
		);
		L.push(`Secure Boot: ${p.secure_boot == null ? 'není k dispozici (legacy BIOS)' : p.secure_boot ? 'zapnutý' : 'VYPNUTÝ'}`);
		L.push(
			`TPM:        ${p.tpm == null ? 'nenalezen' : `${p.tpm[0] ? 'zapnutý' : 'vypnutý'}${p.tpm[1] ? ', specifikace ' + p.tpm[1] : ''}`}`
		);
		L.push(
			`UAC:        ${p.uac_enabled ? 'zapnuté' : 'VYPNUTÉ'}${p.uac_admin_prompt != null ? `, režim výzvy ${p.uac_admin_prompt}` : ''}`
		);
		for (const e of p.encryption ?? []) {
			L.push(`BitLocker:  ${e[0]} — ${e[1] === 1 ? 'zapnuté' : e[1] === 0 ? 'VYPNUTÉ' : 'neznámé'}`);
		}

		const perms = d.security.permissions ?? [];
		const totals = new Map();
		for (const [app, cap, secs] of d.permTotals ?? []) totals.set(`${cap}|${app}`, secs);
		S(`Oprávnění aplikací (${perms.length} záznamů)`);
		L.push(
			'  schopnost            stav        používá  naposledy            30 dnů     aplikace'
		);
		for (const x of perms) {
			const t = totals.get(`${x.capability}|${x.app}`);
			L.push(
				`  ${pad(x.capability, 20)} ${pad(x.allow ? 'povoleno' : x.enforced ? 'zablokováno' : 'odepřeno*', 11)} ${pad(x.in_use ? 'ANO' : '', 8)} ${pad(x.last_used ? ts(x.last_used) : '—', 20)} ${pad(dur(t), 10)} ${x.app_name || x.app}`
			);
		}
		L.push('  * u klasických aplikací Windows „odepřeno" tvrdě nevynucují');
	}

	// ── Uživatelé ──
	if (d.users) {
		const u = d.users;
		H('ÚČTY');
		L.push(`Přihlášený účet: ${u.current_user || '—'}`);
		L.push(`Skupina správců: ${u.admin_group || '—'}`);
		L.push('');
		L.push('  jméno                    celé jméno               vlastnosti                               poslední přihlášení   přihlášení');
		for (const a of u.users ?? []) {
			const flags = [
				a.admin ? 'správce' : 'běžný',
				a.disabled ? 'zakázaný' : null,
				a.locked ? 'zamčený' : null,
				a.microsoft ? 'účet Microsoft' : null,
				a.password_not_required ? 'heslo nevyžadováno' : null
			]
				.filter(Boolean)
				.join(', ');
			L.push(
				`  ${pad(a.name, 24)} ${pad(a.full_name ? "(skryto)" : "", 24)} ${pad(flags, 40)} ${pad(a.last_logon ? ts(a.last_logon) : 'nikdy', 21)} ${num(a.logons, 6)}`
			);
		}
		if ((u.foreign_admins ?? []).length) {
			L.push('');
			L.push('Správci, kteří nejsou lokálním účtem (doména, Entra):');
			for (const f of u.foreign_admins) {
				L.push(`  ${pad(f.name, 40)} ${pad(f.kind, 16)} ${clean(f.sid)}`);
			}
		}
	}

	// ── Síť ──
	if (d.conn) {
		const c = d.conn;
		H('PŘIPOJENÍ');
		for (const a of c.adapters ?? []) {
			L.push(
				`  ${pad(a.name, 34)} ${pad(a.kind, 10)} ${a.up ? 'nahoře' : 'dole'}  ${a.link_mbps ? a.link_mbps + ' Mb/s' : ''}  ${a.dhcp ? 'DHCP' : 'statická'}`
			);
			if (a.description) L.push(`      ${a.description}`);
			if (a.mac) L.push(`      MAC ${maskMac(a.mac)}`);
			if ((a.ips ?? []).length) L.push(`      IP: ${maskList(a.ips)}`);
			if ((a.gateways ?? []).length) L.push(`      brána: ${maskList(a.gateways)}`);
			if ((a.dns ?? []).length) L.push(`      DNS: ${maskList(a.dns)}`);
		}
		// Jméno Wi-Fi sítě se v záznamu neobjeví vůbec. V databázích
		// wardrivingu se dá přeložit na adresu domu — je to nejcitlivější
		// údaj z celého souboru a k diagnostice není potřeba.
		if (c.wifi_connection) {
			const w = c.wifi_connection;
			L.push(
				`  Wi-Fi: připojeno k ${w.secured ? 'zabezpečené' : 'NEZABEZPEČENÉ'} síti (jméno skryto), signál ${w.signal_pct} %`
			);
		}
		if ((c.wifi_networks ?? []).length) {
			const open = c.wifi_networks.filter((w) => !w.secured).length;
			L.push(
				`  V dosahu ${c.wifi_networks.length} sítí (${open} nezabezpečených) — jména skryta`
			);
		}
	}
	if (d.net?.length) {
		S(`Spojení podle aplikací (${d.net.length})`);
		L.push('  aplikace                       procesů  spojení  naslouchá   ↓ B/s     ↑ B/s');
		for (const a of d.net) {
			L.push(
				`  ${pad(a.app_name, 30)} ${num(a.proc_count, 7)} ${num(a.established, 8)} ${num(a.listening, 10)} ${num(a.rx_bps, 9)} ${num(a.tx_bps, 9)}`
			);
			for (const k of a.conns ?? []) {
				// Jméno protistrany zůstává — říká, s jakou SLUŽBOU se
				// mluví, což je jádro téhle sekce, a o uživateli samo
				// o sobě nevypovídá. Adresy jsou maskované.
				L.push(
					`      ${pad(k.proto, 5)} ${pad(maskIp(k.local) + ':' + k.local_port, 22)} → ${pad(maskIp(k.remote) + (k.remote_port ? ':' + k.remote_port : ''), 22)} ${pad(k.state, 12)} ${k.remote_name ?? ''}`
				);
			}
		}
	}

	// ── Po spuštění ──
	if (d.startup?.length) {
		const third = d.startup.filter((x) => !x.system);
		const sys = d.startup.filter((x) => x.system);
		H(`PO SPUŠTĚNÍ (${third.length} třetích stran, ${sys.length} systémových)`);
		L.push('(„start" je nastavený typ spuštění, „běží" je stav právě teď —');
		L.push(' u služeb to nejsou totéž: automatická služba může být zastavená)');
		L.push('');
		L.push('  zdroj          start    běží     položka');
		for (const x of third) {
			L.push(`  ${pad(x.source, 14)} ${pad(startLabel(x), 8)} ${pad(runLabel(x), 8)} ${clean(x.name)}`);
			L.push(`      ${clean(x.command)}`);
		}
		L.push('');
		L.push('Systémové položky (jen výčet, přepnout je Winsent nedovolí):');
		for (const x of sys) {
			L.push(
				`  ${pad(x.source, 14)} ${pad(startLabel(x), 8)} ${pad(runLabel(x), 8)} ${clean(x.name)} — ${x.system_reason ?? ''}`
			);
		}
	}

	// ── Programy (bez cest) ──
	if (d.apps?.length) {
		H(`NAINSTALOVANÉ PROGRAMY (${d.apps.length})`);
		L.push('(bez instalačních cest — mapa souborů do záznamu nepatří)');
		L.push('');
		L.push('  název                                    vydavatel                  verze          instalováno');
		for (const a of d.apps) {
			L.push(
				`  ${pad(a.display_name, 40)} ${pad(a.publisher, 26)} ${pad(a.version, 14)} ${pad(day(a.install_ts), 11)}${a.missing_install ? ' [instalace chybí na disku]' : ''}`
			);
		}
	}

	// ── Procesy teď ──
	if (d.procs?.length) {
		H(`BĚŽÍCÍ PROCESY (${d.procs.length})`);
		L.push('   PID  rodič  název                          CPU%    GPU%    RAM MB   čtení B/s   zápis B/s  vláken  aplikace');
		const sorted = [...d.procs].sort((a, b) => (b.cpu_pct ?? 0) - (a.cpu_pct ?? 0));
		for (const p of sorted) {
			L.push(
				`${String(p.pid).padStart(6)} ${String(p.parent_pid).padStart(6)}  ${pad(p.name, 30)} ${num(p.cpu_pct, 6, 1)} ${num(p.gpu_pct, 6, 1)} ${num((p.ws_bytes ?? 0) / 1048576, 9, 0)} ${num(p.disk_r_bps, 11)} ${num(p.disk_w_bps, 11)} ${num(p.threads, 7)}  ${p.app_name ?? ''}`
			);
		}
	}

	// ── Incidenty ──
	H('INCIDENTY');
	if (d.incidents?.length) {
		L.push(`Vlastní záznamy hlídače (${d.incidents.length}):`);
		for (const i of d.incidents) {
			L.push(`  ${ts(i.ts)}  ${pad(i.kind, 12)} viník: ${i.culprit ?? '—'}`);
			if (i.detail) L.push(`      ${clean(i.detail)}`);
		}
	} else {
		L.push('Vlastní záznamy hlídače: žádné.');
	}
	if (d.crashes?.length) {
		L.push('');
		L.push(`Hlášení o pádech z protokolu Windows (${d.crashes.length}):`);
		for (const c of d.crashes) {
			L.push(`  ${ts(c.ts)}  ${c.app}${c.repeats > 1 ? `  (${c.repeats}x)` : ''}`);
			if (c.summary) L.push(`      ${clean(c.summary)}`);
			if (c.detail) L.push(`      ${clean(c.detail)}`);
		}
	}

	// ── Události a zásahy za 24 h ──
	if (d.events?.length) {
		H(`UDÁLOSTI ZA POSLEDNÍCH 24 HODIN (${d.events.length})`);
		for (const e of d.events) {
			L.push(`  ${ts(e.ts)}  ${pad(e.kind, 16)} ${e.pid != null ? 'pid ' + e.pid : ''}  ${clean(e.detail)}`);
		}
	}
	if (d.audit?.length) {
		H(`ZÁSAHY DO SYSTÉMU (${d.audit.length})`);
		L.push('  kdy                   akce            třída  verdikt  výsledek  cíl');
		for (const a of d.audit) {
			L.push(
				`  ${pad(ts(a.ts), 21)} ${pad(a.action, 15)} ${pad(a.class, 6)} ${pad(a.verdict, 8)} ${pad(a.outcome ?? '', 9)} ${clean(a.target)}`
			);
			if (a.deny_reason) L.push(`      důvod odmítnutí: ${clean(a.deny_reason)}`);
		}
	}

	// ── Zdraví sběračů a spotřeba ──
	if (d.health) {
		H('ZDRAVÍ SBĚRAČŮ');
		const h = d.health;
		L.push(`  procesů ve vzorku: ${h.proc_count}`);
		L.push(`  poslední vzorek:   ${ts(h.last_sample_ts)}`);
		L.push(`  uptime služby:     ${h.uptime_s} s`);
		L.push(`  protokol o běhu:   ${clean(h.log_path)}`);
		if ((h.degraded ?? []).length) {
			L.push('  omezené sběrače:');
			for (const [what, why] of h.degraded) L.push(`      ${pad(what, 24)} ${why}`);
		} else {
			L.push('  omezené sběrače:   žádné');
		}
	}
	if (d.selfUsage) {
		L.push('');
		L.push(
			`Spotřeba Winsentu: CPU ${d.selfUsage.cpu_pct?.toFixed(2)} %, RAM ${Math.round((d.selfUsage.ws_bytes ?? 0) / 1048576)} MB, databáze ${Math.round((d.selfUsage.db_bytes ?? 0) / 1048576)} MB`
		);
	}

	// ── Průběh za 24 h ──
	if (d.sysHist?.length) {
		H(`PRŮBĚH SYSTÉMU ZA 24 HODIN (${d.sysHist.length} vzorků)`);
		L.push('Rozlišení klesá se stářím: čerstvé vzorky po sekundě, starší');
		L.push('průměry za 10 s a za minutu (retenční kaskáda).');
		L.push('');
		L.push('  čas                     CPU%   RAM MB    GPU%     ↓ B/s      ↑ B/s');
		for (const p of d.sysHist) {
			L.push(
				`  ${pad(ts(p.ts), 21)} ${num(p.cpu_pct, 6, 1)} ${num(p.mem_used_mb, 8)} ${num(p.gpu_pct, 7, 1)} ${num(p.net_rx_bps, 10)} ${num(p.net_tx_bps, 10)}`
			);
		}
	}
	if (d.diskHist?.length) {
		H(`ZATÍŽENÍ DISKŮ ZA 24 HODIN (${d.diskHist.length} vzorků)`);
		L.push('  čas                   disk    čtení B/s   zápis B/s');
		for (const [t, idx, r, w] of d.diskHist) {
			L.push(`  ${pad(ts(t), 21)} ${num(idx, 5)} ${num(r, 12)} ${num(w, 12)}`);
		}
	}

	L.push('');
	L.push(SEP);
	L.push('Vygeneroval Winsent. Všechny údaje pocházejí z tohoto počítače');
	L.push('a nic se nikam neodesílá — co se souborem bude dál, je na tobě.');
	return L.join('\n');
}
