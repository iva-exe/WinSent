// Smí Winsent nabídnout ukončení procesu? JEDNO rozhodnutí pro celé UI:
// kontextové menu v Tasks, štítek „součást Windows" u řádků i varování
// v potvrzovacím dialogu se řídí tímhle, ne každé vlastní podmínkou.
//
// Proč: dřív stačilo, že proces není „critical", a „Ukončit" svítilo
// u dwm, explorer, lsass i desítek svchostů — ze 120 procesů Windows
// bylo zamčených jen 14. Vlastník to řekl jasně: ukončení procesu
// Windows se nemá ani nabízet.
//
// Služba má vlastní kopii pravidel (validate) a poslední slovo; tohle
// je její zrcadlo v UI, ať se nenabízí akce, kterou služba stejně
// odmítne nebo která by rozbila systém. UI cestu k souboru nevidí,
// takže „image pod %SystemRoot%" tu zastupuje identita `os:windows` —
// kaskáda identity ji dává jen binárkám pod SystemRoot s platným
// podpisem Microsoftu.
//
// Relativní import schválně: modul jde pustit i v node bez aliasu $lib.
import { isSystemApp } from './mandatory.js';

/// Nástroje Windows, které si uživatel spouští sám a smí je i zavřít —
/// jinak by zamrzlý Poznámkový blok nebo příkazový řádek nešel ukončit
/// odnikud. Stejný seznam drží služba (PLAN §1). Malými písmeny.
const UZIVATELSKE_NASTROJE = new Set([
	'notepad.exe',
	'mspaint.exe',
	'cmd.exe',
	'powershell.exe',
	'powershell_ise.exe',
	'regedit.exe',
	'mmc.exe',
	'taskmgr.exe',
	'msinfo32.exe',
	'write.exe',
	'charmap.exe',
	'snippingtool.exe'
]);

/// Procesy jádra bez souboru na disku. Prázdné jméno má System Idle
/// (pid 0).
const PSEUDO = new Set([
	'',
	'system',
	'registry',
	'memory compression',
	'memcompression',
	'secure system',
	'idle',
	'system idle process'
]);

/// Text do potvrzovacího dialogu u služby od jiného výrobce. Ukončit ji
/// jde, ale uživatel má vědět, že tím nic „nevypne" natrvalo a že může
/// přijít o funkci programu či ovladače, který ji potřebuje.
export const VAROVANI_SLUZBA =
	'Je to služba — Windows ji může hned znovu spustit a s ní může přestat fungovat část programu nebo ovladače.';

const POPISEK_WINDOWS = 'Součást Windows — Winsent ji neukončuje';
// Chráněný proces nemusí být Windows (antivirus běží jako PPL) — psát
// u něj „součást Windows" by byla nepravda.
const POPISEK_CHRANENY = 'Chráněný proces — Winsent ho neukončuje';
const POPISEK_WINSENT = 'Winsent sám sebe neukončuje';
// Identita `name:` bez pseudoprocesu: služba buď soubor procesu
// nepřečetla, nebo ho ještě neověřila (nováček má do doběhnutí kaskády
// prozatímní `name:` klíč — ze samotného řádku se to rozlišit nedá).
// Ani v jednom případě nejde říct, čí proces to je.
const POPISEK_NEOVERENY = 'Neověřený proces — Winsent ho neukončuje';

/// Verdikt pro jeden proces:
///   { trida: 'never' | 'warn' | 'allow', duvod, popisek, windows }
/// `never` = položka „Ukončit" se vůbec nenabídne, místo ní je neaktivní
/// řádek `popisek`; `duvod` je krátké vysvětlení proč. `warn` = nabídne
/// se, dialog ukáže `VAROVANI_SLUZBA`. `windows` = patří Windows (štítek
/// u řádku).
///
/// `identity_key` null/'' znamená „identitu neznám" (vzorek historie
/// z doby před identitami) — pak se pravidlo „bez souboru" nepoužije,
/// jinak by se každý takový řádek tvářil jako neověřený proces.
export function ukonceniProcesu({ protection = 'user', identity_key = '', name = '' } = {}) {
	const key = (identity_key ?? '').toLowerCase();
	const jmeno = (name ?? '').toLowerCase();
	const bezSouboru = key.startsWith('name:');
	const msixWindows =
		key.startsWith('msix:') &&
		// Proces z balíčku nemá v identitě vydavatele (kaskáda ho u MSIX
		// nevyplňuje). Rozhoduje jen rodina balíčku ze sdíleného seznamu;
		// vydavatel se předává natvrdo pro jistotu, ať by ho podmínka
		// „vydavatel obsahuje microsoft" nezahodila — Start a Hledání pak
		// vypadaly jako cizí aplikace. Omyl tímhle směrem nanejvýš
		// schová akci.
		isSystemApp({ identity_key: key, publisher: 'Microsoft' });
	const jeWindows =
		key === 'os:windows' || msixWindows || (bezSouboru && PSEUDO.has(jmeno));
	const nikdy = (duvod, windows = jeWindows) => ({
		trida: 'never',
		duvod,
		popisek: windows ? POPISEK_WINDOWS : POPISEK_CHRANENY,
		windows
	});

	// Vlastní procesy (služba, UI a jeho WebView2): ukončením by Winsent
	// přestal měřit nebo zmizelo okno, ve kterém se uživatel právě dívá.
	if (key === 'app:winsent') {
		return { trida: 'never', duvod: '', popisek: POPISEK_WINSENT, windows: false };
	}
	if (protection === 'critical') {
		return nikdy('kritický proces — bez něj Windows nefungují');
	}
	if (protection === 'protected') {
		return nikdy('chráněný proces (PPL) — Windows ho ukončit nedovolí');
	}
	if (bezSouboru) {
		return PSEUDO.has(jmeno)
			? nikdy('vnitřní proces jádra Windows')
			: {
					trida: 'never',
					duvod: 'Winsent zatím nezná jeho soubor, a tak ho nemá jak ověřit',
					popisek: POPISEK_NEOVERENY,
					windows: false
				};
	}
	if (key === 'os:windows' && !UZIVATELSKE_NASTROJE.has(jmeno)) {
		return nikdy('proces Windows — bez něj přestane fungovat část systému');
	}
	if (msixWindows) {
		return nikdy('systémová aplikace Windows');
	}
	// Služba od jiného výrobce (běží pod účtem služby). Nabídnout jde,
	// ale s varováním — a v nabídce krátce „služba".
	if (protection === 'system') {
		return { trida: 'warn', duvod: 'služba', popisek: '', windows: false };
	}
	return { trida: 'allow', duvod: '', popisek: '', windows: false };
}

const PORADI = { never: 0, warn: 1, allow: 2 };

/// Nejpřísnější verdikt ze seznamu (pro skupinu procesů jedné
/// aplikace): stačí jeden proces, který se ukončit nesmí, a celý strom
/// se z UI nenabízí. Služba by takový proces ze stromu stejně vyřadila,
/// jenže nabídka „Ukončit X se stromem" by pak slibovala víc, než udělá.
export function nejprisnejsi(verdikty) {
	let nej = null;
	for (const v of verdikty) {
		if (!v) continue;
		if (!nej || PORADI[v.trida] < PORADI[nej.trida]) nej = v;
	}
	return nej ?? { trida: 'allow', duvod: '', popisek: '', windows: false };
}
