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
//
// Každá položka má vlastní klíč, ne jen id widgetu: oddělovačů může
// být na ploše libovolně mnoho a musí jít rozlišit — jak pro Svelte
// (klíčovaný each), tak pro přesouvání a mazání.

import { REGISTR, RADEK, MEZERA, MAX_VYSKA, vychoziRozlozeni } from './registr.js';

const KLIC = 'winsent.home.widgets';

export { RADEK, MEZERA, MAX_VYSKA };

/// Kolik sloupců má mřížka naplno. Dlaždice se udávají v těchhle
/// jednotkách, takže rozložení sedí i po zmenšení okna — jen se přelije.
export const SLOUPCU = 4;

/// Kolik sloupců se vejde teď. Přepisuje to Home podle šířky plochy;
/// dlaždice si podle toho ořízne svůj rozsah, aby ta „přes celou" na
/// úzkém okně nevytlačila mřížku do vodorovného rolování.
export const mrizka = $state({ sloupcu: SLOUPCU });

/// Která dlaždice se zrovna táhne (klíč), ať to vědí i ostatní.
export const tah = $state({ klic: null });

let citac = 0;
function novyKlic(id) {
	citac += 1;
	return `${id}~${citac}`;
}

/// Rozměry ze staršího formátu, kde byla velikost pojmenovaná.
/// Ponechané kvůli tomu, aby uživatel po aktualizaci nepřišel o to,
/// co si poskládal.
const STARE = {
	mala: [1, 2],
	stredni: [2, 2],
	vysoka: [1, 4],
	velka: [2, 4],
	siroka: [4, 4]
};

function omez(w, id, jakoW) {
	const reg = REGISTR[id];
	const min = reg?.min ?? [1, 2];
	if (jakoW) return Math.max(min[0], Math.min(SLOUPCU, Math.round(w) || min[0]));
	return Math.max(min[1], Math.min(MAX_VYSKA, Math.round(w) || min[1]));
}

function ocisti(seznam) {
	if (!Array.isArray(seznam)) return null;
	const videne = new Set();
	const out = [];
	for (const it of seznam) {
		if (!it || typeof it !== 'object') continue;
		const reg = REGISTR[it.id];
		if (!reg) continue;
		// Měřák dává smysl jednou; oddělovač kolikrát chce.
		if (!reg.vice) {
			if (videne.has(it.id)) continue;
			videne.add(it.id);
		}
		const stara = typeof it.velikost === 'string' ? STARE[it.velikost] : null;
		const w = omez(it.w ?? stara?.[0] ?? reg.vychozi[0], it.id, true);
		const h = omez(it.h ?? stara?.[1] ?? reg.vychozi[1], it.id, false);
		out.push({
			klic: typeof it.klic === 'string' && it.klic ? it.klic : novyKlic(it.id),
			id: it.id,
			w,
			h,
			text: typeof it.text === 'string' ? it.text : ''
		});
	}
	// Ať čítač nikdy nevyrobí klíč, který už na ploše je.
	for (const it of out) {
		const n = Number(it.klic.split('~')[1]);
		if (Number.isFinite(n) && n > citac) citac = n;
	}
	return out;
}

function nacti() {
	try {
		const ulozene = ocisti(JSON.parse(localStorage.getItem(KLIC) ?? 'null'));
		// Prázdný seznam je platná volba („nechci nic"), takže se
		// nesmí splést s „ještě nikdy nenastaveno".
		return ulozene ?? ocisti(vychoziRozlozeni());
	} catch {
		return ocisti(vychoziRozlozeni());
	}
}

/// Dlaždice v pořadí, v jakém se kreslí.
export const rozlozeni = $state({ dlazdice: nacti() });

let odlozeny = null;
function uloz(hned = true) {
	// Text oddělovače se ukládá při psaní, takže se zápis do úložiště
	// odkládá — jinak by se serializovalo celé rozložení na každou
	// stisknutou klávesu.
	clearTimeout(odlozeny);
	const zapis = () => {
		try {
			localStorage.setItem(KLIC, JSON.stringify(rozlozeni.dlazdice));
		} catch {
			/* bez úložiště platí volba aspoň do zavření okna */
		}
	};
	if (hned) zapis();
	else odlozeny = setTimeout(zapis, 400);
}

function index(klic) {
	return rozlozeni.dlazdice.findIndex((d) => d.klic === klic);
}

/// Je widget na ploše? (u vícenásobných to nic neomezuje)
export function jeNa(id) {
	return rozlozeni.dlazdice.some((d) => d.id === id);
}

export function pridej(id) {
	const reg = REGISTR[id];
	if (!reg || (!reg.vice && jeNa(id))) return;
	rozlozeni.dlazdice = [
		...rozlozeni.dlazdice,
		{ klic: novyKlic(id), id, w: reg.vychozi[0], h: reg.vychozi[1], text: '' }
	];
	uloz();
}

export function odeber(klic) {
	rozlozeni.dlazdice = rozlozeni.dlazdice.filter((d) => d.klic !== klic);
	uloz();
}

/// Šířka ve sloupcích — nastavuje ji přepínač v hlavičce dlaždice.
export function nastavSirku(klic, w) {
	rozlozeni.dlazdice = rozlozeni.dlazdice.map((d) =>
		d.klic === klic ? { ...d, w: omez(w, d.id, true) } : d
	);
	uloz();
}

/// Výška v řádcích — táhne se za spodní hranu dlaždice.
export function nastavVysku(klic, h) {
	const i = index(klic);
	if (i < 0) return;
	const nova = omez(h, rozlozeni.dlazdice[i].id, false);
	if (nova === rozlozeni.dlazdice[i].h) return;
	rozlozeni.dlazdice = rozlozeni.dlazdice.map((d) => (d.klic === klic ? { ...d, h: nova } : d));
	uloz();
}

/// Nejmenší povolená výška — dlaždice ví, kam až smí táhnout.
export function minVyska(id) {
	return REGISTR[id]?.min?.[1] ?? 2;
}
export function minSirka(id) {
	return REGISTR[id]?.min?.[0] ?? 1;
}

/// Text oddělovače.
export function nastavText(klic, text) {
	rozlozeni.dlazdice = rozlozeni.dlazdice.map((d) => (d.klic === klic ? { ...d, text } : d));
	uloz(false);
}

/// Přesune dlaždici na místo jiné (přetažením).
///
/// Nepřehazuje je: vytáhne taženou z pořadí a vloží ji tam, kde je
/// cílová. Prohození by u dlaždic různých velikostí přeskládalo celou
/// mřížku a uživatel by nepoznal, co vlastně udělal.
export function presunNa(zKlic, naKlic) {
	if (zKlic === naKlic) return false;
	const z = index(zKlic);
	const na = index(naKlic);
	if (z < 0 || na < 0) return false;
	const pole = [...rozlozeni.dlazdice];
	const [vyjmuta] = pole.splice(z, 1);
	pole.splice(na, 0, vyjmuta);
	rozlozeni.dlazdice = pole;
	return true;
}

/// Uloží pořadí po dotažení. Během tahu se s úložištěm nepracuje —
/// mezistavů je při přejetí přes plochu klidně dvacet.
export function ulozPoradi() {
	uloz();
}

/// Vrátí výchozí sadu. Nabízí se v režimu úprav, když si to někdo
/// rozháže a chce začít znovu.
export function obnovVychozi() {
	rozlozeni.dlazdice = ocisti(vychoziRozlozeni());
	uloz();
}
