// Ikony aplikací pro dlaždice.
//
// Sekce si každá vede vlastní cache, protože každá žije jen po dobu
// svého zobrazení. Na Home je dlaždic víc a klidně chtějí ikonu téže
// aplikace (Žrouti, Kdo teď stahuje, Nově přibylo) — společná cache
// tedy šetří jak dotazy, tak překreslování.

import { invoke } from '@tauri-apps/api/core';

/// identity_key → data URL. Čte se přímo v šabloně.
export const ikony = $state({});

/// Kolikrát se o klíč už pokoušelo. Ikona, kterou služba nezná, se
/// nemá zkoušet donekonečna při každém tiku.
const pokusy = new Map();

function naUrl(icon) {
	const c = document.createElement('canvas');
	c.width = icon.w;
	c.height = icon.h;
	const ctx = c.getContext('2d');
	ctx.putImageData(new ImageData(new Uint8ClampedArray(icon.rgba), icon.w, icon.h), 0, 0);
	return c.toDataURL();
}

/// Zajistí ikonu pro klíč. Volá se ze šablony při vykreslení řádku.
export async function chciIkonu(key) {
	if (!key || key.startsWith('pid:') || ikony[key]) return;
	const st = pokusy.get(key) ?? 0;
	if (st >= 3) return;
	pokusy.set(key, st + 1);
	try {
		const icon = await invoke('query_icon', { identityKey: key });
		if (icon) {
			ikony[key] = naUrl(icon);
			pokusy.set(key, 99);
		}
	} catch {
		/* služba mimo — zkusí se při dalším vykreslení */
	}
}
