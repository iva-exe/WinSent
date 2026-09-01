// Rozložení dlaždic na Home — co tam je, v jakém pořadí a jak velké.
//
// Ukládá se do localStorage vedle ostatních zobrazovacích předvoleb.
// Do databáze to nepatří: je to volba jednoho člověka u jednoho okna,
// ne měřená skutečnost o počítači.
//
// Uložený seznam se vždycky protřídí proti registru widgetů. Widget,
// který v nové verzi zmizel, se tiše vyhodí; ten, který přibyl, se
// NEPŘIDÁVÁ sám — jinak by uživateli po každé aktualizaci naskakovaly
// dlaždice, které si nevybral. Nové jsou v nabídce „Přidat".

import { REGISTR, VELIKOSTI, vychoziRozlozeni } from './registr.js';

const KLIC = 'winsent.home.widgets';

/// Kolik sloupců má mřížka naplno. Dlaždice se udávají v těchhle
/// jednotkách, takže rozložení sedí i po zmenšení okna — jen se přelije.
export const SLOUPCU = 4;

/// Kolik sloupců se vejde teď. Přepisuje to Home podle šířky plochy;
/// dlaždice si podle toho ořízne svůj rozsah, aby ta „přes celou" na
/// úzkém okně nevytlačila mřížku do vodorovného rolování.
export const mrizka = $state({ sloupcu: SLOUPCU });

function ocisti(seznam) {
	if (!Array.isArray(seznam)) return null;
	const videne = new Set();
	const out = [];
	for (const it of seznam) {
		if (!it || typeof it !== 'object') continue;
		if (typeof it.id !== 'string' || !REGISTR[it.id]) continue;
		if (videne.has(it.id)) continue;
		videne.add(it.id);
		const dovolene = REGISTR[it.id].velikosti ?? Object.keys(VELIKOSTI);
		const velikost = dovolene.includes(it.velikost)
			? it.velikost
			: (REGISTR[it.id].vychozi ?? dovolene[0]);
		out.push({ id: it.id, velikost });
	}
	return out;
}

function nacti() {
	try {
		const ulozene = ocisti(JSON.parse(localStorage.getItem(KLIC) ?? 'null'));
		// Prázdný seznam je platná volba („nechci nic"), takže se
		// nesmí splést s „ještě nikdy nenastaveno".
		return ulozene ?? vychoziRozlozeni();
	} catch {
		return vychoziRozlozeni();
	}
}

/// Dlaždice v pořadí, v jakém se kreslí.
export const rozlozeni = $state({ dlazdice: nacti() });

function uloz() {
	try {
		localStorage.setItem(KLIC, JSON.stringify(rozlozeni.dlazdice));
	} catch {
		/* bez úložiště platí volba aspoň do zavření okna */
	}
}

/// Je widget na ploše?
export function jeNa(id) {
	return rozlozeni.dlazdice.some((d) => d.id === id);
}

export function pridej(id) {
	if (!REGISTR[id] || jeNa(id)) return;
	const w = REGISTR[id];
	rozlozeni.dlazdice = [
		...rozlozeni.dlazdice,
		{ id, velikost: w.vychozi ?? (w.velikosti ?? Object.keys(VELIKOSTI))[0] }
	];
	uloz();
}

export function odeber(id) {
	rozlozeni.dlazdice = rozlozeni.dlazdice.filter((d) => d.id !== id);
	uloz();
}

/// Přepne velikost na další povolenou. Kolotoč místo nabídky: velikostí
/// jsou čtyři a klikat se má rychle.
export function dalsiVelikost(id) {
	const w = REGISTR[id];
	if (!w) return;
	const dovolene = w.velikosti ?? Object.keys(VELIKOSTI);
	rozlozeni.dlazdice = rozlozeni.dlazdice.map((d) => {
		if (d.id !== id) return d;
		const i = dovolene.indexOf(d.velikost);
		return { ...d, velikost: dovolene[(i + 1) % dovolene.length] };
	});
	uloz();
}

/// Přesune dlaždici na jinou pozici (přetažením).
export function presun(zId, naId) {
	if (zId === naId) return;
	const pole = [...rozlozeni.dlazdice];
	const z = pole.findIndex((d) => d.id === zId);
	const na = pole.findIndex((d) => d.id === naId);
	if (z < 0 || na < 0) return;
	const [vyjmuta] = pole.splice(z, 1);
	pole.splice(na, 0, vyjmuta);
	rozlozeni.dlazdice = pole;
	uloz();
}

/// Vrátí výchozí sadu. Nabízí se v režimu úprav, když si to někdo
/// rozháže a chce začít znovu.
export function obnovVychozi() {
	rozlozeni.dlazdice = vychoziRozlozeni();
	uloz();
}
