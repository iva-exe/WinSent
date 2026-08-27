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

/// Načte seznam. Rozbitý nebo cizí obsah se tiše zahodí — jde
/// o pohodlí, ne o data, o která by se dalo přijít.
export function nacti() {
	try {
		const raw = localStorage.getItem(KLIC);
		if (!raw) return [];
		const v = JSON.parse(raw);
		return Array.isArray(v) ? v.filter((it) => it && it.key && it.name) : [];
	} catch {
		return [];
	}
}

/// Přidá položku na začátek; stejnou (podle `key`) posune, nezdvojí.
/// Vrací nový seznam, ať volající nemusí znovu číst úložiště.
export function zapamatuj(item) {
	if (!item?.key) return nacti();
	const zaznam = {
		kind: item.kind,
		key: item.key,
		name: item.name,
		sub: item.sub ?? '',
		path: item.path ?? '',
		identity_key: item.identity_key ?? '',
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

/// Vyprázdní seznam (nabízí se v kontextovém menu).
export function zapomen() {
	try {
		localStorage.removeItem(KLIC);
	} catch {
		/* nic; seznam zůstane, jak byl */
	}
	return [];
}
