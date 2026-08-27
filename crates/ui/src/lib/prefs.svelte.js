// Zobrazovací předvolby UI pro celou aplikaci. Nejde o nastavení
// systému — Winsent nic nepřepíná ani neskrývá před Windows; tohle
// říká jen to, co se má v aplikaci ukazovat.
//
// Stav je $state, aby se přepnutí v Settings projevilo okamžitě i na
// stránce, která je zrovna otevřená. Ukládá se do localStorage, aby
// volba přežila restart. Modul se vyhodnocuje jen ve WebView (SSR i
// prerender jsou v +layout.js vypnuté), takže localStorage tu je.
import { SEKCE } from '$lib/sections.js';

const KEY = 'winsent.prefs';

const DEFAULTS = {
	// Prázdné (0bajtové) soubory bývají legitimní dočasné soubory
	// aplikací — zámky, značky, rozdělaná stahování. Nabízet je jako
	// „smetí k úklidu" by svádělo mazat cizí funkční věci, proto se
	// ve výchozím stavu neukazují.
	showZeroByte: false,

	// Startovací položky, které patří Windows, se neukazují. Přepnout
	// se nedají tak jako tak (zakazuje to validační vrstva ve službě),
	// takže by to byl jen dlouhý seznam řádků k ničemu — a mezi nimi by
	// zapadlo to, co uživatel opravdu ovlivnit může. Zapnout jde
	// k náhledu.
	showSystemStartup: false,

	// Sekce, které se v aplikaci neukazují (cesty jako '/network').
	// Je to čistě zobrazení: nic se tím nevypíná, služba měří dál
	// a data zůstávají — jen se to, co uživatel nepoužívá, neplete
	// do cesty. Ve výchozím stavu je zapnuté všechno.
	hiddenSections: []
};

function load() {
	try {
		const v = { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? '{}') };
		// Poškozený obsah nesmí shodit navigaci — ta se ze seznamu
		// skrytých sekcí odvozuje při každém vykreslení.
		v.hiddenSections = Array.isArray(v.hiddenSections)
			? v.hiddenSections.filter((h) => typeof h === 'string')
			: [];
		return v;
	} catch {
		return { ...DEFAULTS };
	}
}

export const prefs = $state(load());

export function setPref(name, value) {
	prefs[name] = value;
	try {
		localStorage.setItem(KEY, JSON.stringify({ ...prefs }));
	} catch {
		/* bez úložiště volba platí aspoň do zavření okna */
	}
}

/// Ukazuje se sekce v aplikaci?
export function sekceViditelna(href) {
	return !prefs.hiddenSections.includes(href);
}

/// Zapne nebo vypne jednu sekci.
export function prepniSekci(href, zapnout) {
	const bez = prefs.hiddenSections.filter((h) => h !== href);
	setPref('hiddenSections', zapnout ? bez : [...bez, href]);
}

/// Kam jít, když se má otevřít sekce, kterou si uživatel vypnul.
///
/// Nastavení je poslední záchrana: vypnout ho nejde, takže se z prázdné
/// navigace dá vždycky dostat zpátky.
export function prvniViditelnaSekce(preferovana = '/tasks') {
	if (sekceViditelna(preferovana)) return preferovana;
	return SEKCE.find((s) => sekceViditelna(s.href))?.href ?? '/settings';
}
