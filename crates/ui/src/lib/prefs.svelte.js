// Zobrazovací předvolby UI pro celou aplikaci. Nejde o nastavení
// systému — Winsent nic nepřepíná ani neskrývá před Windows; tohle
// říká jen to, co se má v aplikaci ukazovat.
//
// Stav je $state, aby se přepnutí v Settings projevilo okamžitě i na
// stránce, která je zrovna otevřená. Ukládá se do localStorage, aby
// volba přežila restart. Modul se vyhodnocuje jen ve WebView (SSR i
// prerender jsou v +layout.js vypnuté), takže localStorage tu je.
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
	showSystemStartup: false
};

function load() {
	try {
		return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? '{}') };
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
