// Co uživatel naposledy otevřel nebo spustil přes vyhledávání.
//
// Když je řádek pro hledání prázdný, není co ukázat — a prázdná plocha
// po vyvolání lišty působí, jako by se nic nenačetlo. Poslední položky
// jsou navíc to, co člověk otevírá znovu; tohle je zkratka, ne archiv.
//
// Ukládá se do `localStorage`, protože seznam patří k oknu, ne
// k databázi: služba běží pod SYSTEM a nemá co vědět, co si uživatel
// otvírá. Hlavní okno i spotlight lišta jsou dva webview téhož původu,
// takže sdílejí totéž úložiště — položka otevřená v liště se objeví
// i v sekci.

const KLIC = 'winsent.search.recent';

/// Kolik se pamatuje. Padesát je zhruba to, co uživatel pozná jako
/// „nedávno"; delší seznam už jen zdržuje čtení.
export const STROP = 50;

/// Druhy položek, které seznam umí vykreslit.
const DRUHY = new Set(['app', 'file', 'dir']);

/// Načte seznam. Rozbitý nebo cizí obsah se tiše zahodí — jde
/// o pohodlí, ne o data, o která by se dalo přijít.
///
/// Kontroluje se PŘÍSNĚ, včetně druhu a duplicit: co projde sem, kreslí
/// se pak bez dalšího ověřování. Chybějící `kind` by shodilo vykreslení
/// řádku a dvě položky se stejným `key` jsou ve Svelte tvrdá chyba i
/// v produkci — v obou případech by z jednoho poškozeného záznamu
/// v úložišti zmizela celá sekce vyhledávání.
export function nacti() {
	try {
		const raw = localStorage.getItem(KLIC);
		if (!raw) return [];
		const v = JSON.parse(raw);
		if (!Array.isArray(v)) return [];
		const videne = new Set();
		const out = [];
		for (const it of v) {
			if (!it || typeof it !== 'object') continue;
			if (typeof it.key !== 'string' || !it.key) continue;
			if (typeof it.name !== 'string' || !it.name) continue;
			if (!DRUHY.has(it.kind)) continue;
			if (videne.has(it.key)) continue;
			videne.add(it.key);
			out.push({
				kind: it.kind,
				key: it.key,
				name: it.name,
				sub: typeof it.sub === 'string' ? it.sub : '',
				path: typeof it.path === 'string' ? it.path : '',
				identity_key: typeof it.identity_key === 'string' ? it.identity_key : '',
				aumid: typeof it.aumid === 'string' ? it.aumid : '',
				attrs: Number.isFinite(it.attrs) ? it.attrs : 0,
				disk: typeof it.disk === 'string' ? it.disk : '',
				ts: Number.isFinite(it.ts) ? it.ts : 0
			});
		}
		// Od naposledy otevřeného. Pořadí v uloženém poli tomu sice
		// odpovídá, ale zapisují do něj dvě okna (sekce i lišta) a
		// spoléhat se na ně by znamenalo spoléhat se na to, které z nich
		// psalo dřív. Ořezává se AŽ potom, ať se nezahodí ta novější.
		out.sort((a, b) => b.ts - a.ts);
		return out.slice(0, STROP);
	} catch {
		return [];
	}
}

/// Přidá položku na začátek; stejnou (podle `key`) posune, nezdvojí.
/// Vrací nový seznam, ať volající nemusí znovu číst úložiště.
export function zapamatuj(item) {
	if (!item?.key || !DRUHY.has(item.kind)) return nacti();
	const zaznam = {
		kind: item.kind,
		key: item.key,
		name: item.name,
		sub: item.sub ?? '',
		path: item.path ?? '',
		identity_key: item.identity_key ?? '',
		// Bez AUMID by se program z historie spouštěl zase dohledáváním
		// podle jména — a to je přesně to, co umí trefit jinou aplikaci.
		aumid: item.aumid ?? '',
		attrs: item.attrs ?? 0,
		disk: item.disk ?? '',
		ts: Date.now()
	};
	const seznam = [zaznam, ...nacti().filter((it) => it.key !== zaznam.key)].slice(0, STROP);
	try {
		localStorage.setItem(KLIC, JSON.stringify(seznam));
	} catch {
		/* plné úložiště — seznam se prostě neuloží */
	}
	return seznam;
}

/// Odebere jednu položku. Vrací nový seznam.
///
/// Existuje vedle `zapomen`, protože „tohle sem nepatří" je něco
/// jiného než „zapomeň všechno": kvůli jednomu omylem otevřenému
/// souboru nemá uživatel přijít o celou historii.
export function zapomenJednu(key) {
	const seznam = nacti().filter((it) => it.key !== key);
	try {
		localStorage.setItem(KLIC, JSON.stringify(seznam));
	} catch {
		/* plné úložiště — seznam se prostě neuloží */
	}
	return seznam;
}

/// Vyprázdní seznam (nabízí se v kontextovém menu).
export function zapomen() {
	try {
		localStorage.removeItem(KLIC);
	} catch {
		/* nic; seznam zůstane, jak byl */
	}
	return [];
}
