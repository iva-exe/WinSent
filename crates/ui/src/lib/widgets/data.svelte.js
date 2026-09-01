// Jeden zdroj dat pro všechny dlaždice na Home.
//
// Dlaždic může být na obrazovce deset a půlka z nich chce totéž —
// kdyby se každá ptala sama, jelo by přes pipe deset dotazů za sekundu
// na jedna a tatáž data. Widget si proto jen řekne, co potřebuje,
// a dostane hotovou hodnotu; o dotazování se stará tenhle modul.
//
// Sady se počítají na odběratele: první přihlášený rozjede časovač,
// poslední odhlášený ho zastaví. Dlaždice, kterou si uživatel odebral,
// tak přestane stát cokoliv.
//
// Když je okno schované do oznamovací oblasti, WebView2 se uspí a
// časovače se utlumí samy (viz `uspi_webview` v hostiteli). Po
// probuzení se každá sada jednou dotáhne, aby dlaždice neukazovaly
// stav z doby, kdy se uživatel díval naposledy.

import { invoke } from '@tauri-apps/api/core';

/// Popis jedné datové sady: jak se získá a jak často se obnovuje.
///
/// Intervaly nejsou od oka: metriky se mění každou sekundu, inventář
/// aplikací po instalaci. Ptát se na inventář každou sekundu by byl
/// stejný nesmysl jako ptát se na vytížení procesoru jednou za hodinu.
const SADY = {
	system: { interval: 1000, nacti: () => invoke('query_system') },
	procs: { interval: 2000, nacti: () => invoke('query_procs') },
	network: { interval: 5000, nacti: () => invoke('query_network') },
	volumes: { interval: 60_000, nacti: () => invoke('query_volumes') },
	incidents: { interval: 30_000, nacti: () => invoke('query_incidents', { limit: 200 }) },
	// Hardware má ve službě cache s pětisekundovým TTL, takže každý
	// dotaz za tou hranicí stojí plných ~80 ms a 66 kB. Na přehledu
	// stačí jednou za pět minut — deska ani baterie se rychleji nemění.
	hardware: { interval: 300_000, nacti: () => invoke('query_hardware') },
	security: { interval: 120_000, nacti: () => invoke('query_security') },
	// query_startup služba necachuje vůbec (~350 ms po pipe pokaždé),
	// takže se ptát častěji než jednou za pět minut nemá smysl.
	startup: { interval: 300_000, nacti: () => invoke('query_startup') },
	apps: { interval: 300_000, nacti: () => invoke('query_apps') },
	displays: { interval: 300_000, nacti: () => invoke('query_displays') },
	cleanup: { interval: 60_000, nacti: () => invoke('query_cleanup') },
	sysInfo: { interval: 600_000, nacti: () => invoke('query_sys_info') },
	permUse: { interval: 300_000, nacti: () => invoke('query_perm_use_totals', { days: 7 }) },
	drivers: { interval: 600_000, nacti: () => invoke('query_drivers') },
	connection: { interval: 30_000, nacti: () => invoke('query_connection') },
	users: { interval: 300_000, nacti: () => invoke('query_users') },
	audit: { interval: 120_000, nacti: () => invoke('query_audit', { limit: 300 }) },
	samotny: { interval: 5000, nacti: () => invoke('query_self_usage') }
};

/// Poslední známé hodnoty. `null` = ještě se nenačetlo, což si dlaždice
/// hlídají samy a kreslí místo toho kostru.
export const data = $state(Object.fromEntries(Object.keys(SADY).map((k) => [k, null])));

/// Chyby po sadách. Dlaždice díky tomu umí říct „služba neodpovídá"
/// místo toho, aby mlčky ukazovala prázdno.
export const chyby = $state(Object.fromEntries(Object.keys(SADY).map((k) => [k, null])));

/// Kolik vzorků drží živý průběh — čtyři minuty po sekundě.
const STROP = 240;

/// Průběh systémových metrik pro dlaždice s grafem.
///
/// Nesbírá se to z historie na každý tik (to by byl dotaz do databáze
/// každou sekundu), ale z týchž vzorků, které stejně chodí do dlaždic.
/// Historie se čte jednou při otevření a pak už jen po probuzení okna,
/// aby v křivce nezůstala díra po době, kdy bylo okno schované.
export const serie = $state({ ts: [], cpu: [], ramMb: [], gpu: [], rx: [], tx: [] });

function pridejVzorek(s) {
	if (!s) return;
	const ts = Math.floor(Date.now() / 1000);
	if (serie.ts.length && ts <= serie.ts[serie.ts.length - 1]) return;
	serie.ts.push(ts);
	serie.cpu.push(s.cpu_pct ?? 0);
	serie.ramMb.push(s.mem_used_mb ?? 0);
	serie.gpu.push(s.gpu_pct ?? 0);
	serie.rx.push(s.net_rx_bps ?? 0);
	serie.tx.push(s.net_tx_bps ?? 0);
	if (serie.ts.length > STROP) {
		for (const k of Object.keys(serie)) serie[k].splice(0, serie[k].length - STROP);
	}
}

let dopl = false;

/// Dotáhne průběh z historie a slije ho s tím, co už je v paměti.
async function doplnHistorii() {
	if (dopl) return;
	dopl = true;
	try {
		const to = Math.floor(Date.now() / 1000);
		const body = await invoke('query_system_history', { from: to - STROP, to });
		if (Array.isArray(body) && body.length) {
			const nove = { ts: [], cpu: [], ramMb: [], gpu: [], rx: [], tx: [] };
			for (const p of body) {
				nove.ts.push(p.ts);
				nove.cpu.push(p.cpu_pct ?? 0);
				nove.ramMb.push(p.mem_used_mb ?? 0);
				nove.gpu.push(p.gpu_pct ?? 0);
				nove.rx.push(p.net_rx_bps ?? 0);
				nove.tx.push(p.net_tx_bps ?? 0);
			}
			// Historie se bere jako základ a živé vzorky se na ni jen
			// dosypou. Kdyby se místo toho jen předřazovala k tomu, co
			// je v paměti, zůstala by v křivce díra po době, kdy bylo
			// okno schované a časovače utlumené — přesně ta, kterou
			// tohle dotažení má zavřít.
			const posledni = nove.ts[nove.ts.length - 1];
			const od = serie.ts.findIndex((t) => t > posledni);
			if (od >= 0) {
				for (const k of Object.keys(serie)) nove[k].push(...serie[k].slice(od));
			}
			for (const k of Object.keys(serie)) serie[k] = nove[k].slice(-STROP);
		}
	} catch {
		/* bez historie se křivka jen naplní za pochodu */
	}
	dopl = false;
}

const stav = new Map(); // klíč → { pocet, timer, bezi }

async function tik(klic) {
	const s = stav.get(klic);
	if (!s || s.bezi) return;
	s.bezi = true;
	try {
		data[klic] = await SADY[klic].nacti();
		chyby[klic] = null;
		if (klic === 'system') pridejVzorek(data.system);
	} catch (e) {
		chyby[klic] = String(e);
	}
	s.bezi = false;
}

/// Přihlásí odběr datové sady. Vrací funkci pro odhlášení.
///
/// Volá se z `$effect` dlaždice, takže odhlášení proběhne samo, když
/// dlaždici uživatel odebere nebo odejde ze sekce.
export function odebirej(klice) {
	for (const k of klice) {
		if (!SADY[k]) continue;
		const s = stav.get(k) ?? { pocet: 0, timer: null, bezi: false };
		s.pocet += 1;
		if (s.pocet === 1) {
			s.timer = setInterval(() => tik(k), SADY[k].interval);
			tik(k);
			if (k === 'system') doplnHistorii();
		}
		stav.set(k, s);
	}
	return () => {
		for (const k of klice) {
			const s = stav.get(k);
			if (!s) continue;
			s.pocet -= 1;
			if (s.pocet <= 0) {
				clearInterval(s.timer);
				stav.delete(k);
			}
		}
	};
}

/// Dotáhne všechny odebírané sady hned — po probuzení okna.
export function dohon() {
	for (const k of stav.keys()) tik(k);
	if (stav.has('system')) doplnHistorii();
}
