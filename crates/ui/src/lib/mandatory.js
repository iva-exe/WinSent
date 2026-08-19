// Jedna definice „tohle je součást Windows / povinná věc" pro CELOU
// aplikaci (Tasks, Programs, Files, Po spuštění). Když se heuristika
// upraví, upraví se všude — proto to není nakopírované po stránkách.

/// Rodiny MSIX balíčků, které jsou součástí systému.
const SYS_FAMILIES = [
	'microsoft.windows',
	'microsoftwindows',
	'microsoft.vclibs',
	'microsoft.net',
	'microsoft.ui.xaml',
	'microsoft.windowsappruntime',
	'microsoft.sechealthui',
	'microsoft.aad',
	'microsoft.accountscontrol',
	'microsoft.lockapp',
	'microsoft.win32webviewhost',
	'microsoft.windowsstore',
	'microsoft.storepurchaseapp',
	'microsoft.desktopappinstaller'
];

/// Názvy, které znamenají runtime/systémovou komponentu.
const SYS_NAME_HINTS = [
	'visual c++',
	'.net',
	'webview2',
	'universal crt',
	'windows software development kit',
	'windows sdk',
	'update health',
	'directx'
];

/// Systémové cesty — cokoliv pod nimi patří Windows.
const SYS_PATHS = ['c:\\windows\\', '\\system32\\', '\\syswow64\\', '\\winsxs\\'];

/// Je aplikace (identity_key + jméno + vydavatel) součástí Windows?
export function isSystemApp({ identity_key = '', display_name = '', publisher = '' } = {}) {
	const key = identity_key.toLowerCase();
	// OS skupina z identity kaskády je systém z definice.
	if (key.startsWith('os:')) return true;
	const pub = (publisher ?? '').toLowerCase();
	if (!pub.includes('microsoft')) return false;
	if (key.startsWith('msix:')) {
		const fam = key.slice(5);
		return SYS_FAMILIES.some((f) => fam.startsWith(f));
	}
	const n = (display_name ?? '').toLowerCase();
	return SYS_NAME_HINTS.some((h) => n.includes(h));
}

/// Je cesta systémová (Files, mapa souborů, startup položky)?
export function isSystemPath(path = '') {
	const p = (path ?? '').toLowerCase();
	return SYS_PATHS.some((s) => p.includes(s));
}

// ── Ochrana cest: co na disku patří Windows a nesmí se mazat ──
// Největší soubory na disku jsou skoro vždy systémové (odkládací
// soubor, výpis paměti, úložiště ovladačů). Bez vysvětlení to
// v žebříčku vypadá jako „tady je ten žrout místa" a uživatel smaže
// něco, po čem systém nenastartuje. Proto je důvod u cesty, ne
// schovaný v nápovědě.

/// Soubory v KOŘENI svazku, které drží jádro. Řeší se zvlášť a dřív
/// než obecné `.sys`: pagefile.sys taky končí na .sys, ale ovladač to
/// není a hláška o modré obrazovce by u něj mátla.
const ROOT_SYSTEM_FILES = {
	'pagefile.sys':
		'odkládací soubor Windows (virtuální paměť) — systém ho drží otevřený a spravuje sám, smazat se nedá',
	'swapfile.sys':
		'odkládací soubor pro aplikace z Microsoft Storu — patří k pagefile.sys, systém ho spravuje sám',
	'hiberfil.sys':
		'soubor hibernace a rychlého spuštění — velikost odpovídá části paměti RAM, zmizí až po vypnutí hibernace',
	'dumpstack.log': 'pracovní soubor pro zápis výpisu při pádu systému — drží ho jádro Windows',
	'dumpstack.log.tmp': 'pracovní soubor pro zápis výpisu při pádu systému — drží ho jádro Windows',
	bootmgr: 'zavaděč Windows — bez něj počítač nenastartuje',
	bootnxt: 'zaváděcí soubor Windows — bez něj počítač nenastartuje',
	'bootsect.bak': 'záloha zaváděcího sektoru — patří ke startu systému'
};

/// Pravidla se zkoušejí SHORA DOLŮ a vyhrává PRVNÍ shoda. Pořadí je
/// proto součást zadání: konkrétní věci (výpisy paměti, stažené
/// aktualizace) musí být nad obecným „cokoliv ve Windows", jinak by je
/// spolklo a uživatel by se nedozvěděl, že zrovna tohle uklidit jde.
/// `test` dostane cestu malými písmeny se zpětnými lomítky a bez
/// lomítka na konci, plus samotný název souboru.
const PATH_RULES = [
	{
		level: 'mandatory',
		test: (p) =>
			/^[a-z]:\\\$(mft|mftmirr|logfile|volume|attrdef|bitmap|boot|badclus|secure|upcase|extend)(\\|:|$)/.test(
				p
			),
		why: 'vnitřní evidence souborového systému NTFS — nepatří žádnému programu a smazat ji nejde'
	},
	{
		level: 'managed',
		test: (p) => p.includes('\\system volume information'),
		why: 'body obnovení a stínové kopie — místo uvolní Ochrana systému nebo Vyčištění disku, ručně se tam nedostaneš'
	},
	{
		level: 'managed',
		test: (p) => p.includes('\\$recycle.bin') || p.includes('\\recycler'),
		why: 'koš — soubory tu čekají na vysypání koše, mazat složku samotnou nemá smysl'
	},
	{
		level: 'managed',
		test: (p) =>
			/^[a-z]:\\(windows\.old|\$windows\.~bt|\$windows\.~ws|\$winreagent|\$getcurrent|\$sysreset|esd)(\\|$)/.test(
				p
			),
		why: 'zbytek předchozí instalace Windows — uvolní ho Vyčištění disku, sám zmizí do deseti dnů'
	},
	{
		level: 'mandatory',
		test: (p) => /^[a-z]:\\recovery(\\|$)/.test(p) || p.includes('\\recovery\\windowsre'),
		why: 'prostředí pro opravu Windows (WinRE) — odsud se spouští obnovení a oprava startu'
	},
	{
		level: 'mandatory',
		test: (p) =>
			/^[a-z]:\\(boot|efi)(\\|$)/.test(p) ||
			p.includes('\\efi\\microsoft\\boot') ||
			p.includes('\\windows\\boot\\'),
		why: 'zaváděcí soubory — bez nich Windows nenastartují'
	},
	{
		level: 'managed',
		test: (p) =>
			p.endsWith('\\windows\\memory.dmp') ||
			p.includes('\\windows\\minidump') ||
			p.includes('\\windows\\livekernelreports'),
		why: 'výpis paměti po pádu systému — slouží jen k diagnostice, uvolní ho Vyčištění disku'
	},
	{
		level: 'managed',
		test: (p) => p.includes('\\windows\\softwaredistribution'),
		why: 'stažené aktualizace Windows — uklidí je Vyčištění disku, ruční mazání rozbije historii aktualizací'
	},
	{
		level: 'managed',
		test: (p) =>
			p.includes('\\windows\\temp\\') ||
			p.includes('\\windows\\systemtemp') ||
			p.includes('\\appdata\\local\\temp\\'),
		why: 'dočasné soubory — uvolní je Nastavení → Systém → Úložiště nebo Vyčištění disku'
	},
	{
		level: 'managed',
		test: (p) =>
			p.includes('\\windows\\logs\\') ||
			p.includes('\\windows\\panther') ||
			p.includes('\\windows\\prefetch') ||
			p.includes('\\system32\\logfiles') ||
			p.includes('\\system32\\winevt\\logs'),
		why: 'protokoly Windows — čistí je Prohlížeč událostí nebo Vyčištění disku'
	},
	// Kořenové .sys už vyřešila tabulka výše, takže tady zbývají
	// opravdu jen jaderné moduly — i mimo Windows (antipodvody,
	// virtualizace) je to pořád ovladač.
	{
		level: 'mandatory',
		test: (p, name) => name.endsWith('.sys'),
		why: 'ovladač zařízení (.sys) — jádro ho zavádí při startu, po smazání může systém skončit modrou obrazovkou'
	},
	{
		level: 'mandatory',
		test: (p) => p.includes('\\windows\\winsxs'),
		why: 'úložiště komponent Windows — většinu tvoří odkazy na tytéž soubory, reálně zabírá méně; uklízí ho jen DISM'
	},
	{
		level: 'mandatory',
		test: (p) => p.includes('\\windows\\installer'),
		why: 'instalační mezipaměť Windows Installeru — bez ní přestanou jít programy opravit a odinstalovat'
	},
	{
		level: 'mandatory',
		test: (p) => p.includes('\\system32\\driverstore'),
		why: 'úložiště ovladačů — odsud si Windows berou ovladač při každém připojení zařízení'
	},
	{
		level: 'mandatory',
		test: (p) => p.includes('\\system32\\config\\'),
		why: 'registr Windows — otevřený po celou dobu běhu systému'
	},
	{
		level: 'mandatory',
		test: (p) => /^[a-z]:\\windows(\\|$)/.test(p) || SYS_PATHS.some((s) => p.includes(s)),
		why: 'součást Windows — do systémové složky ruční zásah nepatří'
	},
	{
		level: 'mandatory',
		test: (p) => p.includes('\\windowsapps\\') || p.endsWith('\\windowsapps'),
		why: 'instalace aplikací z Microsoft Storu — složku vlastní systém, aplikace se odebírají v Nastavení'
	},
	{
		level: 'mandatory',
		test: (p, name) =>
			name.startsWith('ntuser.dat') || name.startsWith('usrclass.dat') || name === 'ntuser.ini',
		why: 'registr přihlášeného uživatele — Windows ho drží otevřený po celou dobu přihlášení'
	},
	{
		level: 'mandatory',
		test: (p) => p.includes('\\windows defender\\'),
		why: 'Microsoft Defender — složku hlídá ochrana před neoprávněnou manipulací'
	},
	{
		level: 'mandatory',
		test: (p) => /^[a-z]:\\config\.msi(\\|$)/.test(p),
		why: 'pracovní složka Instalační služby Windows — používá se během instalací a oprav'
	},
	{
		level: 'managed',
		test: (p) => p.includes('\\windows\\wer\\') || p.includes('\\appdata\\local\\crashdumps'),
		why: 'hlášení o pádech aplikací — systém je nepotřebuje, uvolní je Vyčištění disku'
	},
	{
		level: 'managed',
		test: (p) => p.includes('\\programdata\\package cache'),
		why: 'mezipaměť instalátorů (Visual Studio, .NET, Visual C++) — bez ní nepůjde program opravit ani odinstalovat'
	},
	{
		level: 'managed',
		test: (p) => /^[a-z]:\\msocache(\\|$)/.test(p),
		why: 'instalační mezipaměť Office — odstraní ji až odinstalace nebo oprava Office'
	},
	{
		level: 'managed',
		test: (p, name) => name === 'ext4.vhdx',
		why: 'disk linuxové distribuce ve WSL — je v něm všechno, co v distribuci máš; odebírá se příkazem wsl --unregister'
	},
	{
		level: 'managed',
		test: (p) =>
			p.includes('\\dockerdesktopwsl') ||
			p.includes('\\docker\\windowsfilter') ||
			p.includes('\\programdata\\dockerdesktop'),
		why: 'data Dockeru — místo uvolní docker system prune, smazání vezme i všechny obrazy a kontejnery'
	},
	{
		level: 'managed',
		test: (p, name) => name.endsWith('.vhdx') || name.endsWith('.avhdx') || name.endsWith('.vhd'),
		why: 'virtuální disk — je v něm celý obsah virtuálního stroje, zmenšuje se v jeho správci, ne smazáním'
	},
	{
		level: 'managed',
		test: (p, name) => name.endsWith('.edb') || name === 'edb.log',
		why: 'databáze systémové služby (index hledání, aktualizace) — zmenší ji přestavění indexu v Možnostech indexování'
	},
	{
		level: 'managed',
		test: (p, name) => name.endsWith('.ost'),
		why: 'offline kopie poštovní schránky — Outlook si ji vytvoří znovu, odstraňuje se v nastavení účtu'
	}
];

/// Co je tahle cesta zač? `null` = běžná uživatelská data, jinak
/// `{ level, reason }`:
///   'mandatory' — povinná součást Windows, smazat nejde nebo se tím
///                 systém rozbije,
///   'managed'   — systémové, ale uklidit to jde — správným nástrojem
///                 Windows, ne ručním smazáním.
/// Vysvětlení je česky a celou větou, protože jde rovnou do UI.
/// Návratová hodnota je nadmnožina `isSystemPath`: co je označené dnes,
/// zůstane označené (viz pravidlo se `SYS_PATHS`). Samotný
/// `isSystemPath` se ale ZÁMĚRNĚ nepředělává — používá ho i Po spuštění
/// k rozhodnutí, jestli je celá skupina systémová, a širší definice by
/// tam změnila význam.
export function systemPathInfo(path = '') {
	const p = (path ?? '')
		.replace(/\//g, '\\')
		.toLowerCase()
		.replace(/\\+$/, '');
	if (!p) return null;
	const name = p.slice(p.lastIndexOf('\\') + 1);
	// Kořen svazku má přednost: pagefile a hiberfil bývají úplně
	// největší soubory na disku a zaslouží si přesnou hlášku.
	if (/^[a-z]:\\[^\\]+$/.test(p) && ROOT_SYSTEM_FILES[name]) {
		return { level: 'mandatory', reason: ROOT_SYSTEM_FILES[name] };
	}
	for (const r of PATH_RULES) {
		if (r.test(p, name)) return { level: r.level, reason: r.why };
	}
	return null;
}
