// Které incidenty si uživatel schoval z přehledu.
//
// Sdílený stav, ne dvě nezávislá čtení localStorage. Skrývá se v sekci
// Incidents, ale závisí na tom i odznak v navigaci — a ten musí zhasnout
// hned při kliknutí, ne až s příštím dotazem po minutě. Dvě kopie téhle
// pravdy by se navíc rozešly: uživatel by položku skryl a v navigaci by
// mu dál svítila, což je přesně to, kvůli čemu ji schovával.
//
// Je to jen zobrazení. Služba o skrývání nic neví a vědět nemá —
// incident se nemaže, jen se přestane ukazovat.

const KLIC = 'winsent.hiddenCrashes';

function nacti() {
	try {
		const a = JSON.parse(localStorage.getItem(KLIC) ?? '[]');
		return new Set(Array.isArray(a) ? a.filter((x) => typeof x === 'string') : []);
	} catch {
		return new Set();
	}
}

/// Klíče skrytých řádků. Obal kvůli tomu, aby šla množina vyměnit celá
/// — Svelte na změnu uvnitř `Set` nereaguje.
export const skryte = $state({ klice: nacti() });

/// Je řádek skrytý?
export function jeSkryty(key) {
	return skryte.klice.has(key);
}

/// Klíč, pod kterým se skrývá náš vlastní incident. Musí sedět s tím,
/// co skládá sekce Incidents — proto je definice tady, na jednom místě.
export function klicIncidentu(id) {
	return `i${id}`;
}

export function skryj(key) {
	const s = new Set(skryte.klice);
	s.add(key);
	zapis(s);
}

export function odkryj(key) {
	const s = new Set(skryte.klice);
	s.delete(key);
	zapis(s);
}

function zapis(s) {
	skryte.klice = s;
	try {
		localStorage.setItem(KLIC, JSON.stringify([...s]));
	} catch {
		/* bez úložiště to platí aspoň do zavření okna */
	}
}
