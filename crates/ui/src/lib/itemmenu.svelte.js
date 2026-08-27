// Kontextové menu položky — sdílený stav pro celou aplikaci.
//
// Menu se vykresluje JEDNOU v layoutu, ne v každé sekci. Kdyby si ho
// každá stránka kreslila sama, měli bychom deset skoro stejných menu,
// deset různých zavíracích logik a deset míst, kde se dá zapomenout na
// „Co to je?".
//
// Sekce jen řekne, na co uživatel klikl (`openMenu`), a dodá seznam
// akcí. První položku doplňuje tenhle modul sám — viz `openMenu`.
import { invoke } from '@tauri-apps/api/core';

export const itemMenu = $state({
	/// Je otevřené?
	open: false,
	/// Pozice v okně (px).
	x: 0,
	y: 0,
	/// Nadpis menu — co je ta položka.
	title: '',
	/// Podtitulek (model, cesta, vydavatel) — jen když něco přidává.
	subtitle: '',
	/// Položky: { label, icon, danger?, disabled?, hint?, run() }.
	items: []
});

/// Obecná slova, která samotná nic neřeknou.
///
/// Když se uživatel ptá „co to je", je mu k ničemu vyhledat „Disk"
/// nebo „NVMe". Zajímá ho model, který bývá až v popisku pod názvem —
/// proto se hledá první opravdu konkrétní řetězec z nabídnutých.
const OBECNA = new Set([
	'disk',
	'disky',
	'nvme',
	'ssd',
	'hdd',
	'sata',
	'usb',
	'ram',
	'pamět',
	'paměť',
	'cpu',
	'gpu',
	'procesor',
	'grafika',
	'základní deska',
	'deska',
	'bios',
	'uefi',
	'baterie',
	'síť',
	'sitovy adapter',
	'síťový adaptér',
	'ethernet',
	'wi-fi',
	'wifi',
	'adaptér',
	'adapter',
	'svazek',
	'oddíl',
	'obrazovka',
	'monitor',
	'klávesnice',
	'myš',
	'zvuk',
	'audio',
	'reproduktory',
	'mikrofon',
	'kamera',
	'webkamera',
	'tiskárna',
	'ovladač',
	'driver',
	'služba',
	'service',
	'proces',
	'aplikace',
	'program',
	'soubor',
	'složka',
	'účet',
	'uživatel',
	'neznámé',
	'neznámý',
	'nezjištěno',
	'—',
	'-'
]);

/// Je ten řetězec dost konkrétní na to, aby se dal vyhledat?
///
/// Konkrétní = není v seznamu obecných slov a nese něco, co se dá najít:
/// buď víc slov, nebo číslo (modely bývají „ST2000DM008", „RTX 3070").
function konkretni(s) {
	const t = (s ?? '').trim();
	if (t.length < 3) return false;
	if (OBECNA.has(t.toLowerCase())) return false;
	// Samotné číslo ani samotná přípona nic neřeknou.
	if (/^[\d\s.,%-]+$/.test(t)) return false;
	const slov = t.split(/\s+/).length;
	return slov > 1 || /\d/.test(t) || t.length >= 6;
}

/// Poskládá dotaz pro „Co to je?".
///
/// Bere kandidáty v pořadí, v jakém je sekce nabídla, a vezme první
/// konkrétní. Když je název obecný („NVMe") a model konkrétní
/// („ST2000DM008-2FR102"), spojí je — hledá se pak „NVMe ST2000DM008",
/// což je přesně to, co uživatel chtěl vědět.
///
/// `kontext` je slovo, které se přidá, jen když by dotaz sám o sobě byl
/// dvojznačný (například „svchost.exe" → „svchost.exe proces Windows").
export function dotazNaVyhledani(kandidati, kontext = '') {
	const seznam = (Array.isArray(kandidati) ? kandidati : [kandidati])
		.map((s) => (s ?? '').toString().trim())
		.filter(Boolean);
	if (!seznam.length) return '';

	const prvni = seznam[0];
	const konkretniKandidat = seznam.find(konkretni);

	// Název je obecný, ale máme konkrétnější popis — spojíme je, ať je
	// z dotazu vidět, o jakou věc jde.
	if (!konkretni(prvni) && konkretniKandidat && konkretniKandidat !== prvni) {
		return `${prvni} ${konkretniKandidat}`.trim();
	}
	const zaklad = konkretniKandidat ?? prvni;
	return kontext ? `${zaklad} ${kontext}`.trim() : zaklad;
}

/// Otevře menu u kurzoru.
///
/// `polozky` jsou akce sekce. Položku „Co to je?" přidává tenhle modul
/// vždycky a jako první — je to jediná věc, která má být na každém
/// řádku aplikace stejně.
export function openMenu(event, { title, subtitle = '', hledat, kontext = '', items = [] }) {
	event.preventDefault();
	event.stopPropagation();

	const dotaz = dotazNaVyhledani(hledat ?? [title, subtitle], kontext);
	const prvni = {
		label: 'Co to je?',
		icon: 'help',
		hint: dotaz,
		disabled: !dotaz,
		run: () => invoke('search_web', { query: dotaz })
	};

	itemMenu.title = title ?? '';
	itemMenu.subtitle = subtitle ?? '';
	itemMenu.items = [prvni, ...items.filter(Boolean)];
	itemMenu.x = event.clientX;
	itemMenu.y = event.clientY;
	itemMenu.open = true;
}

export function closeMenu() {
	itemMenu.open = false;
	itemMenu.items = [];
}

// ── Akce, které se opakují napříč sekcemi ──────────────────────────
//
// Aby se v deseti stránkách nepsalo desetkrát totéž a nerozešlo se to
// v popiscích.

/// Zkopíruje text do schránky.
export function akceKopirovat(text, label = 'Kopírovat název') {
	return {
		label,
		icon: 'copy',
		disabled: !text,
		run: () => navigator.clipboard.writeText(String(text ?? ''))
	};
}

/// Otevře složku v Průzkumníku a soubor v ní označí.
export function akceOtevritUmisteni(path) {
	return {
		label: 'Otevřít umístění',
		icon: 'folder',
		disabled: !path,
		run: () => invoke('open_path', { path })
	};
}

/// Vyhledá vlastní dotaz (když „Co to je?" nestačí — třeba chybová
/// hláška nebo kód pádu).
export function akceHledat(dotaz, label = 'Vyhledat') {
	return {
		label,
		icon: 'search',
		hint: dotaz,
		disabled: !dotaz,
		run: () => invoke('search_web', { query: dotaz })
	};
}

/// Oddělovač mezi skupinami akcí.
export const oddelovac = { separator: true };
