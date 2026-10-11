// Barvy zátěže a teplot — a formát čísel, ze kterého se barva čte.
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

/// Barva podle zátěže (0–100) — stejné zastávky jako gradient grafu.
export function colorForLoad(v) {
	if (v == null) return 'var(--text-dim)';
	if (v <= 55) return rgb(OK_C);
	if (v <= 75) return rgb(lerpC(OK_C, WARN_C, (v - 55) / 20));
	if (v <= 90) return rgb(lerpC(WARN_C, DANGER_C, (v - 75) / 15));
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
	if (pct == null || pct <= 55 || text === '—') return null;
	if (pct <= 75) return rgb(lerpC(zaklad, WARN_C, (pct - 55) / 20));
	if (pct <= 90) return rgb(lerpC(WARN_C, DANGER_C, (pct - 75) / 15));
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
