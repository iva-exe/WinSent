// Zvětšení rozhraní — totéž, co dělá Ctrl+kolečko v prohlížeči.
//
// Používá CSS `zoom` na kořenovém elementu: na rozdíl od zvětšení
// písma škáluje i rámečky, ikony a odsazení, takže se rozvržení
// nerozpadne. Hodnota se drží v localStorage, protože je to čistě
// zobrazovací předvolba jednoho uživatele na jednom stroji — nemá co
// dělat v konfiguraci služby.

const KEY = 'winsent.ui-scale';
/// Meze držíme rozumné: pod 90 % je text nečitelný, nad 150 % se do
/// okna nevejdou tabulky.
export const MIN = 90;
export const MAX = 150;
export const DEFAULT = 110;
export const STEPS = [90, 100, 110, 125, 150];

/// Aktuální zvětšení v procentech.
export const scale = $state({ value: DEFAULT });

function clamp(n) {
	return Math.min(MAX, Math.max(MIN, Math.round(n)));
}

/// Načte uloženou hodnotu a použije ji. Volá se jednou při startu UI.
export function initScale() {
	let v = DEFAULT;
	try {
		const saved = parseInt(localStorage.getItem(KEY) ?? '', 10);
		if (Number.isFinite(saved)) v = clamp(saved);
	} catch {
		/* localStorage nedostupné — jede se na výchozí hodnotě */
	}
	apply(v);
}

/// Nastaví zvětšení, uloží ho a hned použije.
export function setScale(v) {
	apply(clamp(v));
}

function apply(v) {
	scale.value = v;
	try {
		localStorage.setItem(KEY, String(v));
	} catch {
		/* neuložíme — na funkci to nemá vliv */
	}
	if (typeof document !== 'undefined') {
		document.documentElement.style.zoom = v / 100;
	}
}
