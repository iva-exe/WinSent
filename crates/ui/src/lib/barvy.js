// Barvy zátěže, teplot a stavu (disky, signál) — a formát čísel, ze
// kterého se barva čte.
//
// Dřív si každé místo neslo vlastní stupnici: Home měl schody bez
// zastávky na 75 %, Tasks gradient, a teplota procesoru 72 °C vyšla
// v Tasks červenooranžově, na Home oranžově a v Hardwaru zeleně. Totéž
// číslo mělo podle stránky jiný význam. Bydlí to proto tady, jako zátěž
// systému v sysload.js — dvě kopie by se rozešly.
//
// RGB konstanty odpovídají app.css (--ok, --warn, --danger, --text-dim,
// --text).

const OK_C = [74, 222, 128];
const WARN_C = [245, 158, 11];
const DANGER_C = [239, 68, 68];
const TEXT_C = [154, 154, 161]; // --text-dim, výchozí barva buňky
/// --text: výchozí barva čísel, která ztlumená nejsou (seznam v Incidentech).
export const TEXT_JASNY = [236, 236, 239];

const lerpC = (c1, c2, t) => c1.map((v, i) => Math.round(v + (c2[i] - v) * t));
const rgb = (c) => `rgb(${c[0]}, ${c[1]}, ${c[2]})`;

// ── zátěž ────────────────────────────────────────────────────────
// Zastávky v procentech: do první zelená, ke druhé přechod do oranžové,
// ke třetí do červené, nad ní červená. Čtou je čísla (colorForLoad,
// barvaBunky) i grafy (GRADIENT_ZATEZE) — křivka má v dané výšce tutéž
// barvu jako číslo se stejnou hodnotou. Dřív měl každý graf zastávky
// opsané zvlášť a malé grafy na Home žádné.
export const ZATEZ = [55, 75, 90];
const [Z_OK, Z_WARN, Z_DANGER] = ZATEZ;

/// Zastávky svislého gradientu grafů zátěže: [poloha na ose 0–100 %
/// jako 0–1, barva]. Barva je jméno proměnné z app.css bez „--" — každý
/// graf si ji čte po svém (průhlednost výplně se liší).
export const GRADIENT_ZATEZE = [
	[0, 'ok'],
	[Z_OK / 100, 'ok'],
	[Z_WARN / 100, 'warn'],
	[Z_DANGER / 100, 'danger'],
	[1, 'danger']
];

/// Barva podle zátěže (0–100) — stejné zastávky jako gradient grafu.
export function colorForLoad(v) {
	if (v == null) return 'var(--text-dim)';
	if (v <= Z_OK) return rgb(OK_C);
	if (v <= Z_WARN) return rgb(lerpC(OK_C, WARN_C, (v - Z_OK) / (Z_WARN - Z_OK)));
	if (v <= Z_DANGER) return rgb(lerpC(WARN_C, DANGER_C, (v - Z_WARN) / (Z_DANGER - Z_WARN)));
	return rgb(DANGER_C);
}

// Barvy čísel v tabulkách a seznamech. Stejné zastávky jako graf
// (55 / 75 / 90 %), jen místo zelené zůstává výchozí barva buňky —
// barevně tak vystoupí jen to, co už je v oranžové a červené. Nula je
// zešedlá, ať se oko zastaví u řádků, které něco dělají.
//
// Nula tak, jak ji tabulka napíše: „0.0 %", „0.0 MB", „0 kB/s", „0".
const NULA = /^0(\.0+)?( |$)/;
/// `null` = nechat výchozí barvu buňky. `pct` je zátěž přímo, u paměti
/// podíl z celku; kde celek není (I/O, síť), stačí null — pak jen šedne nula.
/// `zaklad` je tatáž výchozí barva v RGB. Přechod do oranžové musí začít
/// z ní: v jasném seznamu by jinak 56 % vypadalo tmavší než 54 %.
export function barvaBunky(text, pct, zaklad = TEXT_C) {
	if (NULA.test(text)) return 'var(--text-faint)';
	if (pct == null || pct <= Z_OK || text === '—') return null;
	if (pct <= Z_WARN) return rgb(lerpC(zaklad, WARN_C, (pct - Z_OK) / (Z_WARN - Z_OK)));
	if (pct <= Z_DANGER) return rgb(lerpC(WARN_C, DANGER_C, (pct - Z_WARN) / (Z_DANGER - Z_WARN)));
	return rgb(DANGER_C);
}

// ── teploty ──────────────────────────────────────────────────────
// Jedna stupnice pro celou aplikaci: [varování, horko] ve °C. Hranice
// jsou z Hardwaru, jediného místa, které rozlišovalo druh součástky —
// NVMe disk má 60 °C už teplých, procesor ve hře běžně víc.
export const TEPLOTY = { cpu: [75, 88], gpu: [75, 88], disk: [55, 70] };

/// Schod pro štítky a text: 'cool' | 'warm' | 'hot'.
export function tempTrida(c, druh) {
	const [warn, hot] = TEPLOTY[druh];
	return c >= hot ? 'hot' : c >= warn ? 'warm' : 'cool';
}

/// Tentýž schod jako barva.
export function barvaTeploty(c, druh) {
	const t = tempTrida(c, druh);
	return t === 'hot' ? 'var(--danger)' : t === 'warm' ? 'var(--warn)' : 'var(--ok)';
}

/// Barva teploty pro dlaždice v Tasks — TENTÝŽ schod jako štítky
/// v Hardwaru a widget Teploty, ne plynulý přechod. S přechodem svítilo
/// 72 °C v Tasks skoro oranžově, zatímco štítek u téže hodnoty zeleně.
export function colorForTemp(c, druh = 'cpu') {
	if (c == null) return 'var(--text-dim)';
	return barvaTeploty(c, druh);
}

// ── stav: ok / warn / bad ────────────────────────────────────────
// Plnost svazku, opotřebení SSD a síla signálu nejsou zátěž, ale stav:
// tři schody bez přechodu. Třídy jsou tytéž, jaké mají stránky v CSS
// (pilulky v Hardwaru, Connection), a barva se z nich bere z jedné
// tabulky — pruh, pilulka i odznak v postranním panelu tak u téže
// hodnoty říkají totéž. Dřív měl každý své hranice: odznak svítil
// u 90% svazku oranžově vedle červeného pruhu, signál 65 % byl
// v Connection zelený a na Home oranžový.
const BARVA_STAVU = { ok: 'var(--ok)', warn: 'var(--warn)', bad: 'var(--danger)' };
const PORADI_STAVU = ['ok', 'warn', 'bad'];

/// 'ok' | 'warn' | 'bad' → barva.
export function barvaStavu(t) {
	return BARVA_STAVU[t];
}

/// Horší ze dvou stavů — odznak s plným svazkem i opotřebeným diskem
/// svítí podle toho horšího.
export function horsiStav(a, b) {
	return PORADI_STAVU.indexOf(a) >= PORADI_STAVU.indexOf(b) ? a : b;
}

/// Plnost svazku v %: [varování, plno].
export const ZAPLNENI = [75, 90];

export function zaplneniTrida(pct) {
	return pct >= ZAPLNENI[1] ? 'bad' : pct >= ZAPLNENI[0] ? 'warn' : 'ok';
}

export function barvaZaplneni(pct) {
	return barvaStavu(zaplneniTrida(pct));
}

/// Opotřebení SSD ze SMART (used_pct, výrobcem odhadnutá spotřebovaná
/// životnost): [varování, vyčerpáno]. `critical` jsou bity kritického
/// varování NVMe — cokoli nenulového je problém bez ohledu na procenta.
export const OPOTREBENI = [80, 100];

/// `h` je záznam zdraví disku ze služby; chybějící údaj = v pořádku,
/// jako dřív na všech místech.
export function opotrebeniTrida(h) {
	const u = h?.used_pct ?? 0;
	return h?.critical || u >= OPOTREBENI[1] ? 'bad' : u >= OPOTREBENI[0] ? 'warn' : 'ok';
}

export function barvaOpotrebeni(h) {
	return barvaStavu(opotrebeniTrida(h));
}

/// Síla signálu WiFi v %: [dobrý od, slabý pod]. Hranice jsou ze
/// stránky Connection, která signál ukazuje jako hlavní údaj.
export const SIGNAL = [60, 35];

export function signalTrida(pct) {
	return pct >= SIGNAL[0] ? 'ok' : pct >= SIGNAL[1] ? 'warn' : 'bad';
}

export function barvaSignalu(pct) {
	return barvaStavu(signalTrida(pct));
}

// ── formát ───────────────────────────────────────────────────────
// Tady, ne u dlaždic: barvaBunky pozná nulu podle textu, a tentýž
// proces má mít v Tasks, na Home i v Incidentech totéž číslo. Dřív
// Home psal „1.6 GB" (desítkově) a Tasks u téže aplikace „1.50 GB".

/// Paměť v bajtech, binárně: „512.0 MB", „1.50 GB".
export function fmtMem(bytes) {
	const mb = bytes / (1024 * 1024);
	return mb >= 1024 ? `${(mb / 1024).toFixed(2)} GB` : `${mb.toFixed(1)} MB`;
}

/// Rychlost přenosu: „3.2 MB/s", „120 kB/s".
export function fmtBps(v) {
	if (v == null) return '—';
	const mb = v / (1024 * 1024);
	return mb >= 1 ? `${mb.toFixed(1)} MB/s` : `${(v / 1024).toFixed(0)} kB/s`;
}

// Rozpočet samotného Winsentu (Nastavení → spotřeba nástroje). Nad ním
// je číslo oranžové — v Nastavení i v dlaždici na Home stejně.
export const ROZPOCET = { cpu_pct: 0.5, ws_bytes: 50 * 1024 * 1024 };
