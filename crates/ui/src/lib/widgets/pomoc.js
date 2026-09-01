// Formátování společné všem dlaždicím.
//
// Kdyby si každá dlaždice psala vlastní `fmtSize`, ukazovalo by jich
// pět „1,2 GB" a šestá „1234 MB" — a rozdíl by nešel vysvětlit ničím
// jiným než tím, kdo který widget psal.

/// Barva podle zátěže v procentech. Stejné hranice jako v Tasks.
export function barvaZateze(v) {
	if (v == null) return 'var(--text-dim)';
	if (v <= 55) return 'var(--ok)';
	if (v <= 90) return 'var(--warn)';
	return 'var(--danger)';
}

/// Rychlost přenosu.
export function bps(v) {
	if (v == null) return '—';
	const mb = v / (1024 * 1024);
	if (mb >= 1) return `${mb.toFixed(1)} MB/s`;
	const kb = v / 1024;
	return kb >= 1 ? `${kb.toFixed(0)} kB/s` : '0';
}

/// Velikost v bajtech, na dvě až tři platné číslice.
export function velikost(b) {
	if (b == null) return '—';
	if (b >= 1e12) return `${(b / 1e12).toFixed(1)} TB`;
	if (b >= 1e9) return `${(b / 1e9).toFixed(b >= 1e10 ? 0 : 1)} GB`;
	if (b >= 1e6) return `${(b / 1e6).toFixed(0)} MB`;
	if (b >= 1e3) return `${(b / 1e3).toFixed(0)} kB`;
	return `${b} B`;
}

/// „před 3 h" z unixového času v sekundách.
export function pred(ts) {
	if (!ts) return '—';
	const s = Math.max(0, Math.floor(Date.now() / 1000 - ts));
	if (s < 90) return 'právě teď';
	if (s < 3600) return `před ${Math.floor(s / 60)} min`;
	if (s < 86400) return `před ${Math.floor(s / 3600)} h`;
	const d = Math.floor(s / 86400);
	return d === 1 ? 'včera' : `před ${d} dny`;
}

/// Datum bez času — u instalací a datumů ovladačů je hodina k ničemu.
export function den(ts) {
	if (!ts) return '—';
	return new Date(ts * 1000).toLocaleDateString('cs-CZ');
}

/// Doba trvání ze sekund: „3 h 12 min".
export function doba(s) {
	if (s == null) return '—';
	if (s < 60) return `${Math.round(s)} s`;
	if (s < 3600) return `${Math.floor(s / 60)} min`;
	const h = Math.floor(s / 3600);
	const m = Math.floor((s % 3600) / 60);
	return m ? `${h} h ${m} min` : `${h} h`;
}

/// Kolik procent svazku je zaplněno.
export function zaplneno(v) {
	return v?.total_bytes ? ((v.total_bytes - v.free_bytes) / v.total_bytes) * 100 : 0;
}

/// Skloňování pro počty, které se v dlaždicích píšou slovem.
export function pocet(n, jedna, dva, pet) {
	if (n === 1) return `${n} ${jedna}`;
	if (n >= 2 && n <= 4) return `${n} ${dva}`;
	return `${n} ${pet}`;
}
