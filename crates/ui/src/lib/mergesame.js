// Druhá úroveň slučování: položky, které se jmenují stejně.
//
// První úroveň (group_key ze služby) spojí to, co JE jeden kus hardwaru —
// jedna myš rozepsaná na šestnáct rozhraní. Zůstanou ale řádky, které
// se jmenují úplně stejně a přitom jeden kus nejsou: sedm „PCI HOST
// Bridge", osm „Volume", šest „Motherboard resources". Pro uživatele je
// to sedmkrát tentýž řádek, i když jsou to různé čipy.
//
// Slučují se proto i tyhle — ale POCTIVĚ. Skupina nikdy netvrdí, že je
// to jeden kus: nese počet a pod rozklikem každý kus zvlášť i s tím,
// čím se od sebe liší. Rozdíl proti oprávněním v Security je právě
// tenhle: tam jsou staré verze téže aplikace opravdu jedna věc, tady je
// to N věcí se shodným jménem a musí to být vidět.

/// Sloučí položky se shodným klíčem do jednoho řádku.
///
/// `keyOf(item)` určuje, co patří k sobě (typicky jméno, u ovladačů
/// jméno + verze). `items` zůstávají v původním pořadí; skupina se
/// vykreslí na místě svého prvního člena.
///
/// Vrací pole objektů `{ key, head, members, count }`, kde `head` je
/// první člen (ten se ukazuje na řádku) a `members` jsou všichni.
export function mergeSame(items, keyOf) {
	const map = new Map();
	const order = [];
	for (const it of items) {
		const k = keyOf(it);
		if (!map.has(k)) {
			map.set(k, []);
			order.push(k);
		}
		map.get(k).push(it);
	}
	return order.map((k, i) => {
		const members = map.get(k);
		return {
			// Klíč nese i pořadí: dvakrát stejný klíč ve {#each} je
			// v produkčním buildu Svelte tvrdá chyba, která zabije
			// překreslování celé stránky.
			key: `${i}:${k}`,
			head: members[0],
			members,
			count: members.length
		};
	});
}

/// Čím se členové skupiny od sebe liší — do rozkliku.
///
/// Vrací pole popisků; když se v ničem neliší, vrátí prázdné pole
/// a rozklik nemá co ukázat.
export function differences(members, fields) {
	const out = [];
	for (const f of fields) {
		const vals = new Set(members.map((m) => m[f] ?? '').filter(Boolean));
		if (vals.size > 1) out.push(f);
	}
	return out;
}
