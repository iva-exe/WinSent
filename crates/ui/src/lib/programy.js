// Co je program zač a smí se odsud odinstalovat? Jedno místo pro
// seznam, detail, kontextové menu i potvrzovací dialog v Programs.
//
// Proč: dřív rozhodovalo jen „je to součást Windows" (isSystemApp)
// a všechno ostatní dostalo „Odinstalovat" bez slova. Ovladač grafiky,
// .NET runtime, na kterém běží půlka programů, PawnIO, ze kterého
// Winsent čte teplotu, i jednotlivé aktualizace Office vypadaly jako
// obyčejná aplikace. A Store aplikace měly „Odinstalovat", které služba
// pokaždé odmítla.
//
// Dvě úrovně jistoty, a ty se nesmí míchat:
//   · zámek odinstalace jen z toho, co ví služba (no_remove, is_update,
//     system_package) nebo sdílená definice Windows (isSystemApp) —
//     služba totéž odmítne i sama;
//   · štítky podle jména a vydavatele jsou jen INFORMACE. Heuristika
//     se může splést, a tak nikdy nic nezamyká — jen připíše, co se
//     po odinstalaci pravděpodobně stane.
//
// Relativní import schválně: modul jde pustit i v node bez aliasu $lib.
import { isSystemApp } from './mandatory.js';

const n = (a) => (a?.display_name ?? '').toLowerCase();
const pub = (a) => (a?.publisher ?? '').toLowerCase();

/// Programy, které „opravují" ovladače za tebe. Slovo driver mají
/// v názvu, ale ovladač to není — a štítek „ovladač hardwaru" by u nich
/// lhal.
const DRIVER_UPDATERY =
	/driver ?(booster|easy|pack|max|genius|updater|support|talent|reviver|fix|navigator|identifier|hub|magic|doctor)/;

/// Běhová prostředí a knihovny, bez kterých se jiné programy nespustí.
/// Pořadí nehraje roli, každé pravidlo stojí samo.
const RUNTIME = [
	(s) => s.includes('visual c++') && /redistributable|runtime/.test(s),
	(s) =>
		/(\.net|windows desktop) runtime|\.net core runtime|\.net host|asp\.net core.*(runtime|shared framework)/.test(s),
	(s) => /\.net framework/.test(s) && !/targeting|sdk|developer pack/.test(s),
	(s) => s.includes('xna framework'),
	(s) => s.includes('gameinput'),
	(s) => s.includes('localdb'),
	(s) => /\bjava\b|\bjre\b|\bjdk\b|openjdk|temurin/.test(s),
	// SDK (hlavičky, nástroje pro vývojáře) běhové prostředí není.
	(s) => s.includes('directx') && !s.includes('sdk'),
	(s) => s.includes('webview2'),
	(s) => /windows ?app ?runtime/.test(s),
	(s) => s.includes('universal crt redistributable'),
	(s) => /vulkan ?run ?time/.test(s),
	(s) => s.includes('physx')
];

/// Je to běhové prostředí (runtime, redistributable)?
export function jeRuntime(a) {
	const s = n(a);
	return RUNTIME.some((t) => t(s)) || /windowsappruntime/.test((a?.identity_key ?? '').toLowerCase());
}

/// Je to balík ovladače hardwaru? Podle jména (driver / ovladač) nebo
/// výrobce, který jiné balíky skoro nedělá (Realtek).
function jeOvladac(a) {
	const s = n(a);
	if (DRIVER_UPDATERY.test(s)) return false;
	return /\bdrivers?\b|ovlada[čc]/.test(s) || s.includes('realtek') || pub(a).includes('realtek');
}

/// Je to aktualizace jiného programu? Rozhoduje služba (`is_update`:
/// ParentKeyName / ReleaseType v registru); „(KB123456)" v názvu je
/// třetí z jejích znaků a čte se i tady, ať se aktualizace pozná
/// i se starší službou, která pole ještě neposílá.
function jeAktualizace(a) {
	return !!a?.is_update || /\(kb\d+\)/.test(n(a));
}

/// Patří program Windows (zámek + štítek „součást Windows")? Služba to
/// ví přesněji (system_package = podpis balíčku), sdílená definice
/// z mandatory.js pokrývá zbytek.
export function patriWindows(a) {
	return !!a?.system_package || a?.kind === 'os' || isSystemApp(a ?? {});
}

/// Informační štítek: { druh, stitek, popis, varovani } nebo null.
///   druh    — 'zabezpeceni' | 'winsent' | 'aktualizace' | 'runtime' | 'ovladac'
///   stitek  — krátké slovo do řádku seznamu,
///   popis   — věta do detailu a tooltipu,
///   varovani — věta do potvrzení odinstalace ('' = bez varování).
/// Jeden štítek na program, ten nejdůležitější: dva vedle sebe v úzkém
/// řádku seznamu by se nečetly.
export function infoProgramu(a) {
	if (!a) return null;
	const s = n(a);
	// Bezpečnostní software pozná služba (Security Center) — jistota,
	// proto první a jediný s důrazným varováním.
	if (a.security_product) {
		return {
			druh: 'zabezpeceni',
			stitek: 'zabezpečení',
			popis: 'bezpečnostní software hlášený v Centru zabezpečení Windows',
			varovani:
				'Je to bezpečnostní software. Po odinstalaci zkontroluj v sekci Security, že ochranu převzal Microsoft Defender — do té doby může být počítač bez antiviru.'
		};
	}
	if (s.includes('pawnio')) {
		return {
			druh: 'winsent',
			stitek: 'potřebuje Winsent',
			popis: 'ovladač, ze kterého Winsent čte teplotu CPU — bez něj teplota zmizí',
			varovani: 'Winsent z PawnIO čte teplotu CPU. Po odinstalaci ji přestane ukazovat.'
		};
	}
	if (jeAktualizace(a)) {
		return {
			druh: 'aktualizace',
			stitek: 'aktualizace',
			popis: 'aktualizace programu nebo Windows',
			varovani: ''
		};
	}
	if (jeRuntime(a)) {
		const webview = s.includes('webview2');
		return {
			druh: 'runtime',
			stitek: 'runtime',
			popis: webview
				? 'běhové prostředí — potřebují ho jiné programy a Winsent na něm běží'
				: 'běhové prostředí — potřebují ho jiné programy, bez něj se nespustí',
			varovani: webview
				? 'Na WebView2 běží okno Winsentu i řada dalších programů. Bez něj se nespustí.'
				: 'Je to běhové prostředí pro jiné programy. Ty, které ho potřebují, se po odinstalaci nespustí.'
		};
	}
	if (jeOvladac(a)) {
		const vigem = s.includes('vigem');
		return {
			druh: 'ovladac',
			stitek: 'ovladač',
			popis: vigem
				? 'ovladač virtuálních herních ovladačů — programy, které je vytvářejí, bez něj nefungují'
				: 'ovladač hardwaru — po odinstalaci zařízení přestane fungovat',
			varovani: vigem
				? 'Je to ovladač virtuálních herních ovladačů. Programy, které je vytvářejí, po odinstalaci přestanou fungovat.'
				: 'Je to ovladač hardwaru. Po odinstalaci zařízení přestane fungovat, dokud ovladač znovu nenainstaluješ.'
		};
	}
	return null;
}

/// Smí se program odsud odinstalovat?
///   { smi, kratce, duvod }
/// `kratce` jde do nápovědy v kontextovém menu (řádek je úzký), `duvod`
/// celou větou do detailu — hned za popis ze `infoProgramu`, na který
/// u runtime a aktualizací navazuje (co to je, říká už popis). Zámky
/// odpovídají tomu, co odmítne služba — UI nenabídne akci, která by
/// skončila na „zamítnuto".
export function odinstalace(a) {
	if (!a) return { smi: false, kratce: '', duvod: '' };
	if (patriWindows(a)) {
		// Runtime z Microsoftu (Visual C++, .NET) sdílená definice
		// zamyká, ale součást Windows to není — důvod musí říct pravdu.
		return jeRuntime(a)
			? {
					smi: false,
					kratce: 'potřebují ho programy',
					duvod: 'Winsent ho neodinstalovává.'
				}
			: {
					smi: false,
					kratce: 'patří Windows',
					duvod: 'Součást Windows — neodinstalovává se.'
				};
	}
	if (jeAktualizace(a)) {
		return {
			smi: false,
			kratce: 'aktualizace',
			duvod: 'Winsent ji samostatně neodinstalovává.'
		};
	}
	if (a.no_remove) {
		return {
			smi: false,
			kratce: 'výrobce ji zakázal',
			duvod: 'Výrobce odinstalaci tímhle způsobem zakázal — ani Windows ji v seznamu aplikací nenabízejí.'
		};
	}
	if (a.kind === 'msix') {
		return {
			smi: false,
			kratce: 'odebereš v Nastavení → Aplikace',
			duvod: 'Aplikace ze Storu odebereš v Nastavení → Aplikace.'
		};
	}
	return { smi: true, kratce: '', duvod: '' };
}
