// Jedna definice „tohle je součást Windows / povinná věc" pro CELOU
// aplikaci (Tasks, Programs, Files, Po spuštění). Když se heuristika
// upraví, upraví se všude — proto to není nakopírované po stránkách.

import { PATH_KB, vzor } from './pathkb.js';

/// Rodiny MSIX balíčků, které jsou součástí systému (malými písmeny,
/// porovnává se začátek jména rodiny). STEJNÝ seznam má služba
/// (win_sys::msix::SYSTEM_FAMILY_PREFIXES v crates/win-sys/src/msix.rs)
/// — kdo mění jeden, mění oba,
/// jinak UI zamkne, co služba pustí, nebo naopak.
///
/// Tečka na konci je součást zadání: `microsoft.windows.` chytí
/// Microsoft.Windows.ShellExperienceHost, ale ne Kalkulačku
/// (Microsoft.WindowsCalculator) — ta jde odebrat jako každá jiná
/// aplikace ze Storu a „součást Windows" by o ní lhalo.
const SYS_FAMILIES = [
	'microsoft.windows.',
	'microsoftwindows.',
	'windows.',
	'microsoft.vclibs',
	'microsoft.net.',
	'microsoft.ui.xaml',
	'microsoft.windowsappruntime',
	'microsoft.sechealthui',
	'microsoft.aad.',
	'microsoft.accountscontrol',
	'microsoft.lockapp',
	'microsoft.win32webviewhost',
	'microsoft.asynctextservice',
	'microsoft.bioenrollment',
	'microsoft.creddialoghost',
	'microsoft.ecapp',
	'microsoft.microsoftedgedevtoolsclient',
	'microsoft.xboxgamecallableui',
	'ncsiuwpapp',
	'microsoft.windowsstore',
	'microsoft.storepurchaseapp',
	'microsoft.desktopappinstaller'
];

/// Balíčky Windows pojmenované GUIDem (Průzkumník, tiskový dialog…).
/// Prefix jim dát nejde, poznají se podle tvaru jména.
const SYS_GUID = /^[0-9a-f]{8}-[0-9a-f]{4}-/;

/// Pseudoprocesy jádra. Nemají soubor, takže je kaskáda pozná jen
/// podle jména (`name:system`) — bez tohohle by v Tasks System nebo
/// Memory Compression vypadaly jako neznámá aplikace.
const PSEUDO = new Set([
	'system',
	'registry',
	'memory compression',
	'memcompression',
	'secure system',
	'idle',
	'system idle process'
]);

/// Názvy, které znamenají runtime/systémovou komponentu.
/// Windows SDK tu schválně NENÍ: je to sada pro vývojáře, kterou jde
/// odinstalovat jako každý jiný program, a Windows ji nepotřebují.
const SYS_NAME_HINTS = [
	'visual c++',
	'.net',
	'webview2',
	'universal crt',
	'update health',
	'directx'
];

/// Části sad pro vývojáře. Nesou v názvu slova z nápověd výš („Windows
/// SDK DirectX x64 Remote", „Universal CRT Headers…", „.NET SDK",
/// „ASP.NET Core Targeting Pack"), ale runtime to není a Windows je
/// nepotřebují — bez téhle výjimky by je Programs zamkly jako „součást
/// Windows". Runtime (Universal CRT Redistributable, .NET Runtime) sem
/// nespadá.
const VYVOJARSKE = /\bsdk\b|headers|targeting pack|developer pack|apphost pack|toolset|templates|\btools x(?:64|86)\b/;

/// Systémové cesty — cokoliv pod nimi patří Windows.
const SYS_PATHS = ['c:\\windows\\', '\\system32\\', '\\syswow64\\', '\\winsxs\\'];

/// Je MSIX balíček (jméno rodiny) součástí Windows?
export function isSystemPackage(family = '') {
	const f = (family ?? '').toLowerCase();
	return SYS_GUID.test(f) || SYS_FAMILIES.some((p) => f.startsWith(p));
}

/// Je aplikace (identity_key + jméno + vydavatel) součástí Windows?
export function isSystemApp({ identity_key = '', display_name = '', publisher = '' } = {}) {
	const key = (identity_key ?? '').toLowerCase();
	// OS skupina z identity kaskády je systém z definice.
	if (key.startsWith('os:')) return true;
	if (key.startsWith('name:') && PSEUDO.has(key.slice(5))) return true;
	// U MSIX rozhoduje rodina balíčku, ne vydavatel: běžící procesy ho
	// z kaskády nemají (publisher None), takže dřívější podmínka
	// „vydavatel obsahuje microsoft" v Tasks shodila nabídku Start
	// i Hledání na obyčejnou aplikaci.
	if (key.startsWith('msix:')) return isSystemPackage(key.slice(5));
	const pub = (publisher ?? '').toLowerCase();
	if (!pub.includes('microsoft')) return false;
	const n = (display_name ?? '').toLowerCase();
	if (VYVOJARSKE.test(n)) return false;
	return SYS_NAME_HINTS.some((h) => n.includes(h));
}

/// Je cesta systémová (startup položky)? Záměrně hrubé a beze změny —
/// Po spuštění podle toho rozhoduje o celé skupině; pro soubory na
/// disku slouží `systemPathInfo` níž.
export function isSystemPath(path = '') {
	const p = (path ?? '').toLowerCase();
	return SYS_PATHS.some((s) => p.includes(s));
}

// ── Ochrana cest: co na disku smí pryč a co ne ──────────────────────
// Největší soubory a složky na disku jsou skoro vždy systémové nebo
// „kontejnery" (profil, AppData). Bez vysvětlení to v žebříčku vypadá
// jako „tady je ten žrout místa" a uživatel smaže něco, po čem systém
// nenastartuje nebo přijde o data. Proto je důvod u cesty, ne schovaný
// v nápovědě.
//
// Úrovně (od nejpřísnější):
//   mandatory — součást Windows; nikdy nemazat,
//   managed   — spravuje Windows; místo uvolní jen jmenovaný nástroj
//               Windows, ruční mazání ne,
//   kontejner — Users, profil, AppData, ProgramData, Program Files:
//               velikost je součet obsahu, celé se to nikdy nemaže,
//   aplikace  — patří programu (instalace, jeho data, ovladač);
//               odebírá se odinstalací nebo v programu, ne ručně,
//   data      — tvoje data; smazat jde, ale přijdeš o ně,
//   cache     — mezipaměť z pathkb.js; smazat jde (s jistotou podle
//               dokumentace výrobce),
//   null      — Winsent neví, co to je.
// Do koše smí jen data, cache a null — viz `smiDoKose`.

const SYSROOT = '%systemroot%';
const RE_SYSROOT = vzor(SYSROOT);
const VYCISTENI = 'Vyčištění disku';
const ULOZISTE_TEMP = 'Nastavení → Systém → Úložiště → Dočasné soubory';
const INCIDENTY = ' Smazáním zmizí i podklady k Incidentům ve Winsentu.';

/// Soubory v KOŘENI svazku, které drží Windows. Řeší se zvlášť a dřív
/// než obecné `.sys`: pagefile.sys taky končí na .sys, ale ovladač to
/// není a hláška o modré obrazovce by u něj mátla.
const ROOT_SYSTEM_FILES = {
	'pagefile.sys': {
		why: 'odkládací soubor Windows (virtuální paměť) — systém ho drží otevřený a slouží i k zápisu výpisu po pádu; smazat se nedá',
		tool: 'Vlastnosti systému → Upřesnit → Výkon → Virtuální paměť'
	},
	'swapfile.sys': {
		why: 'odkládací soubor pro aplikace z Microsoft Storu — patří k pagefile.sys, systém ho spravuje sám',
		tool: 'Vlastnosti systému → Upřesnit → Výkon → Virtuální paměť'
	},
	'hiberfil.sys': {
		why: 'soubor hibernace a rychlého spuštění — velký zhruba jako část RAM; zmizí jen po vypnutí hibernace, tím ale přestane fungovat i rychlé spuštění a hybridní spánek',
		tool: 'příkaz powercfg /hibernate off (jako správce)'
	},
	'dumpstack.log': { why: 'pracovní soubor pro zápis výpisu při pádu systému — drží ho jádro Windows' },
	'dumpstack.log.tmp': { why: 'pracovní soubor pro zápis výpisu při pádu systému — drží ho jádro Windows' },
	bootmgr: { why: 'zavaděč Windows — bez něj počítač nenastartuje' },
	bootnxt: { why: 'zaváděcí soubor Windows — bez něj počítač nenastartuje' },
	'bootsect.bak': { why: 'záloha zaváděcího sektoru — patří ke startu systému' },
	'boottel.dat': { why: 'záznam o průběhu startu, který zapisuje zavaděč Windows — patří ke startu systému' },
	'$winre_backup_partition.marker': {
		why: 'značka, kterou nechává aktualizace prostředí pro obnovení (WinRE) — soubor je prázdný, ale patří Windows'
	}
};

/// Pravidla se zkoušejí SHORA DOLŮ a vyhrává PRVNÍ shoda. Pořadí je
/// proto součást zadání: konkrétní věci (výpisy paměti, stažené
/// aktualizace) musí být nad obecným „cokoliv ve Windows", jinak by je
/// spolklo a uživatel by se nedozvěděl, že zrovna tohle uklidit jde.
/// `vzory` viz pathkb.js (sedí na složku i na vše pod ní), `test`
/// dostane cestu malými písmeny se zpětnými lomítky a bez lomítka na
/// konci, plus samotný název souboru.

// 1) Stavba svazku a Windows — nad znalostní bází, protože nic z toho
//    nesmí přebít žádné pravidlo programu.
const PRAVIDLA_WINDOWS = [
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
		vzory: ['%disk%/system volume information'],
		why: 'body obnovení a stínové kopie — místo uvolní Ochrana systému (smazat body nebo snížit limit), ručně se tam nedostaneš',
		tool: 'Ochrana systému → Konfigurovat'
	},
	{
		level: 'managed',
		vzory: ['%disk%/$recycle.bin', '%disk%/recycler'],
		why: 'koš — místo uvolní vysypání koše; složka samotná se nemaže',
		tool: 'Vysypat koš'
	},
	{
		level: 'managed',
		vzory: ['%disk%/windows.old'],
		why: 'předchozí verze Windows po upgradu — sama zmizí po 10 dnech; smazáním se už nejde vrátit zpět a mohou v ní být i tvoje staré soubory (Windows.old\\Users)',
		tool: `${ULOZISTE_TEMP} → Předchozí instalace Windows`
	},
	{
		level: 'managed',
		vzory: ['%disk%/{$windows.~bt,$windows.~ws,$winreagent,$getcurrent,$sysreset}'],
		why: 'pracovní soubory instalace a upgradu Windows — ručně je smazat nejde',
		tool: `${VYCISTENI} → Vyčistit systémové soubory → Dočasné instalační soubory systému Windows`
	},
	{
		level: 'managed',
		vzory: ['%disk%/esd'],
		why: 'instalační soubory Windows pro „Obnovit tento počítač" — po smazání bude muset Windows obnovu stáhnout',
		tool: `${VYCISTENI} → Instalační soubory Windows ESD`
	},
	{
		level: 'mandatory',
		vzory: ['%disk%/recovery'],
		why: 'prostředí pro opravu Windows (WinRE) — odsud se spouští obnovení a oprava startu'
	},
	{
		level: 'mandatory',
		vzory: ['%disk%/{boot,efi}', `${SYSROOT}/boot`],
		why: 'zaváděcí soubory — bez nich Windows nenastartují'
	},
	{
		level: 'mandatory',
		vzory: ['%disk%/config.msi'],
		why: 'pracovní složka Instalační služby Windows — používá se během instalací a oprav'
	},
	// ── %SystemRoot%: konkrétní složky nad obecným „součást Windows" ──
	{
		level: 'managed',
		vzory: [`${SYSROOT}/memory.dmp`],
		why: 'úplný výpis paměti po modré obrazovce — slouží jen k diagnostice.' + INCIDENTY,
		tool: `${VYCISTENI} → Soubory výpisu paměti systémových chyb`
	},
	{
		level: 'managed',
		vzory: [`${SYSROOT}/minidump`],
		why: 'malé výpisy po modré obrazovce — slouží jen k diagnostice, Winsent z nich čte příčinu pádu.' + INCIDENTY,
		tool: `${VYCISTENI} → Soubory minimálního výpisu systémových chyb`
	},
	{
		// Vyčištění disku je NEmaže (žádný handler na tuhle složku) —
		// dřívější text tvrdil opak.
		level: 'managed',
		vzory: [`${SYSROOT}/livekernelreports`],
		why: 'výpisy jádra pořízené za běhu, když se zasekne ovladač (často grafický, složka WATCHDOG) — slouží jen k diagnostice; Vyčištění disku je nemaže, ručně je smaže jen správce'
	},
	{
		level: 'mandatory',
		vzory: [`${SYSROOT}/softwaredistribution/datastore`, `${SYSROOT}/system32/catroot2`],
		why: 'databáze služby Windows Update a katalogy podpisů — sahá se na ně jen při oficiálním resetu Windows Update',
		tool: 'Poradce při potížích se službou Windows Update'
	},
	{
		level: 'managed',
		vzory: [`${SYSROOT}/softwaredistribution/download`],
		why: 'stažené aktualizace Windows — služba Windows Update je po instalaci uklízí sama; ruční smazání patří jen k opravě aktualizací (se zastavenou službou)',
		tool: 'Poradce při potížích se službou Windows Update'
	},
	{
		level: 'managed',
		vzory: [
			`${SYSROOT}/serviceprofiles/networkservice/appdata/local/microsoft/windows/deliveryoptimization`,
			`${SYSROOT}/softwaredistribution/deliveryoptimization`,
			'%disk%/deliveryoptimization'
		],
		why: 'kopie stažených aktualizací a aplikací ze Storu, které Windows může sdílet s jinými počítači — služba je po 3 dnech maže sama a drží je pod 20 % volného místa',
		tool: `${VYCISTENI} → Soubory Optimalizace doručení`
	},
	{
		level: 'managed',
		vzory: [`${SYSROOT}/softwaredistribution`, '%disk%/wudownloadcache'],
		why: 'pracovní složka služby Windows Update — stará stažení uklidí služba sama nebo Vyčištění disku',
		tool: `${VYCISTENI} → Vyčištění aktualizací Windows Update`
	},
	{
		level: 'managed',
		vzory: [`${SYSROOT}/{temp,systemtemp}`],
		why: 'dočasné soubory Windows a služeb — Vyčištění disku bere jen soubory starší než 7 dní, protože novější můžou patřit běžící instalaci',
		tool: VYCISTENI
	},
	{
		level: 'managed',
		vzory: [`${SYSROOT}/logs`, `${SYSROOT}/system32/logfiles`],
		why: 'protokoly Windows — ručně se nemažou, protože do nich služby právě zapisují; staré uklidí Vyčištění disku',
		tool: VYCISTENI
	},
	{
		level: 'managed',
		vzory: [`${SYSROOT}/panther`],
		why: 'protokoly instalace a upgradu Windows — pomáhají řešit potíže s instalací',
		tool: `${VYCISTENI} → Soubory protokolu upgradu systému Windows`
	},
	{
		level: 'managed',
		vzory: [`${SYSROOT}/system32/winevt/logs`],
		why: 'protokoly událostí Windows — mají pevný strop velikosti a nejstarší záznamy se přepisují samy',
		tool: 'Prohlížeč událostí → Vymazat protokol'
	},
	{
		// Prefetch NENÍ protokol a Vyčištění disku na něj nesahá —
		// dřívější text radil obojí. Microsoft čištění výslovně nedoporučuje.
		level: 'managed',
		vzory: [`${SYSROOT}/prefetch`],
		why: 'záznamy, podle kterých Windows zrychluje spouštění programů — Windows je spravuje a omezuje samo; smazáním nic trvale neuvolníš, jen se dočasně zpomalí start'
	},
	{
		level: 'mandatory',
		vzory: [`${SYSROOT}/winsxs`],
		why: 'úložiště součástí Windows — většinu tvoří odkazy na tytéž soubory, takže reálně zabírá méně; ruční mazání může znemožnit start i aktualizace',
		tool: `${VYCISTENI} → Vyčištění aktualizací Windows Update`
	},
	{
		level: 'mandatory',
		vzory: [`${SYSROOT}/installer`],
		why: 'mezipaměť Instalační služby Windows — bez ní nepůjdou programy aktualizovat, opravit ani odinstalovat; soubory jsou pro každý počítač jedinečné a nejdou obnovit odjinud',
		tool: 'odinstalace nepotřebných programů'
	},
	{
		level: 'mandatory',
		vzory: [`${SYSROOT}/system32/driverstore`],
		why: 'úložiště ovladačů — odsud Windows instaluje ovladač při každém připojení zařízení; staré verze uklidí Vyčištění disku a nejnovější nechá',
		tool: `${VYCISTENI} → Balíčky ovladačů zařízení`
	},
	{
		level: 'mandatory',
		vzory: [`${SYSROOT}/system32/config`],
		why: 'registr Windows — otevřený po celou dobu běhu systému'
	},
	{
		level: 'mandatory',
		test: (p, name) => name.endsWith('.sys') && RE_SYSROOT.test(p),
		why: 'ovladač Windows (.sys) — jádro ho zavádí při startu, po smazání může systém skončit modrou obrazovkou'
	},
	{
		level: 'mandatory',
		vzory: [SYSROOT],
		why: 'součást Windows — do systémové složky ruční zásah nepatří'
	},
	{
		// Složka Windows na jiném disku: druhý nainstalovaný systém, nebo
		// zbytek staré instalace. Který z nich, Winsent nepozná — a obojí
		// se ručně nemaže.
		level: 'mandatory',
		vzory: ['%disk%/windows'],
		why: 'složka Windows mimo systémový disk — buď další nainstalovaný Windows, nebo zbytek staré instalace; ručně ji nemaž'
	},
	// ── Windows mimo %SystemRoot% ──
	{
		level: 'mandatory',
		vzory: ['%localappdata%/microsoft/windowsapps'],
		why: 'zástupci, přes které se aplikace ze Storu spouštějí z příkazové řádky — spravuje je Windows'
	},
	{
		level: 'mandatory',
		vzory: ['%disk%/windowsapps', '%programfiles%/windowsapps'],
		why: 'instalace aplikací z Microsoft Storu — složku vlastní systém, aplikace se odebírají v Nastavení',
		tool: 'Nastavení → Aplikace'
	},
	{
		level: 'aplikace',
		vzory: ['%disk%/wpsystem'],
		why: 'data aplikací ze Storu nainstalovaných na tento disk — odeberou se s aplikací v Nastavení',
		tool: 'Nastavení → Aplikace'
	},
	{
		level: 'mandatory',
		test: (p, name) =>
			name.startsWith('ntuser.dat') || name.startsWith('usrclass.dat') || name === 'ntuser.ini',
		why: 'registr uživatele — Windows ho drží otevřený po celou dobu přihlášení'
	},
	{
		level: 'mandatory',
		vzory: [
			'%programdata%/microsoft/windows defender',
			'%programfiles%/{windows defender,windows defender advanced threat protection}'
		],
		why: 'Microsoft Defender — složku hlídá ochrana před neoprávněnou manipulací; postradatelné kopie uklidí Vyčištění disku',
		tool: `${VYCISTENI} → Antivirová ochrana v programu Microsoft Defender`
	},
	{
		level: 'managed',
		vzory: ['%programdata%/microsoft/windows/wer', '%localappdata%/microsoft/windows/wer'],
		why: 'hlášení o chybách pro Microsoft — systém je nepotřebuje a sám je omezuje (archiv nejvýš 1000 hlášení)',
		tool: `${VYCISTENI} → Diagnostika systému Windows na základě hlášení chyb a zpětné vazby`
	},
	{
		level: 'managed',
		vzory: ['%programdata%/microsoft/search'],
		why: 'index vyhledávání Windows — zmenší ho jen přestavění indexu',
		tool: 'Možnosti indexování → Upřesnit → Znovu sestavit'
	},
	{
		level: 'mandatory',
		vzory: ['%programdata%/microsoft'],
		why: 'data Windows a programů Microsoftu pro všechny uživatele (šifrovací klíče, nabídka Start, licence) — ručně se tu nemaže'
	},
	{
		level: 'managed',
		vzory: ['%localappdata%/microsoft/windows/explorer/thumbcache_*.db'],
		why: 'náhledy obrázků a videí v Průzkumníku — vytvoří se znovu při procházení složek',
		tool: `${VYCISTENI} → Miniatury`
	},
	{
		level: 'managed',
		vzory: ['%localappdata%/microsoft/windows/inetcache'],
		why: 'dočasné internetové soubory starého prohlížeče a součástí Windows',
		tool: `${VYCISTENI} → Temporary Internet Files`
	},
	{
		level: 'managed',
		vzory: ['%localappdata%/microsoft/windows/webcache'],
		why: 'databáze historie a mezipaměti součástí Windows — Windows ji drží otevřenou a spravuje sám'
	},
	{
		// Báze ji vede jako bezpečnou (SAFE), koš tu přesto není: čerpají
		// z ní Incidenty ve Winsentu a o smazání má rozhodnout uživatel
		// v Průzkumníku, ne jedno kliknutí v žebříčku. Nástroj Windows na
		// ni není, takže souhrn říká „ručně nemazat", ne „jeho nástroj".
		level: 'managed',
		vzory: ['%localappdata%/crashdumps'],
		why: 'výpisy paměti padlých aplikací (nejvýš 10, nejstarší se přepisují) — systém je nepotřebuje, slouží jen k hledání příčiny pádu; Vyčištění disku je nemaže.' +
			INCIDENTY
	},
	{
		level: 'managed',
		vzory: ['%localappdata%/d3dscache'],
		why: 'mezipaměť shaderů DirectX, kterou spravuje Windows — hry si ji po vyčištění vytvoří znovu',
		tool: `${VYCISTENI} → Mezipaměť shaderů DirectX`
	},
	{
		level: 'managed',
		vzory: ['%disk%/perflogs'],
		why: 'složka Sledování výkonu Windows — bývá prázdná, data v ní spravuje Sledování výkonu',
		tool: 'Sledování výkonu (perfmon)'
	},
	{
		level: 'aplikace',
		vzory: ['%programdata%/package cache'],
		why: 'mezipaměť instalátorů (Visual Studio, .NET, Visual C++ a další) — bez ní nepůjde program opravit, aktualizovat ani odinstalovat',
		tool: 'odinstalace programu, který ji vytvořil'
	},
	{
		level: 'aplikace',
		vzory: ['%disk%/msocache'],
		why: 'instalační mezipaměť Office — odstraní ji až odinstalace nebo oprava Office',
		tool: 'Nastavení → Aplikace → Microsoft Office'
	},
	{
		level: 'aplikace',
		vzory: ['%programdata%/syswatch'],
		why: 'data Winsentu (historie měření, nastavení služby) — smaže je odinstalace Winsentu'
	}
];

// 2) Znalostní báze programů (pathkb.js) se zkouší mezi oběma bloky.
//    Dočasné soubory jsou až za ní: NuGetScratch v %TEMP% je
//    zdokumentovaná mezipaměť, zbytek Tempu spravuje Windows.

// 3) Obecná pravidla: typy souborů, kontejnery, instalace, tvoje data.
const PRAVIDLA_OBECNA = [
	{
		level: 'managed',
		vzory: ['%localappdata%/temp'],
		why: 'dočasné soubory programů — Windows je uklízí samo a bere jen soubory nepoužité déle než 7 dní, protože právě běžící instalace tu mají rozpracované soubory',
		tool: `${ULOZISTE_TEMP}, nebo ${VYCISTENI}`
	},
	{
		level: 'mandatory',
		vzory: ['%localappdata%/microsoft/windows'],
		why: 'data Windows pro tvůj účet (nastavení Průzkumníku, oznámení, historie) — spravuje je Windows'
	},
	// Ovladač mimo Windows (antipodvody, virtualizace, PawnIO) patří
	// programu, ne Windows — dřív dostal štít „součást Windows".
	{
		level: 'aplikace',
		test: (p, name) => name.endsWith('.sys'),
		why: 'soubor ovladače (.sys) — pokud patří nainstalovanému programu, odebírá se jeho odinstalací; ruční smazání může zastavit start počítače',
		tool: 'odinstalace programu v Programech'
	},
	{
		level: 'aplikace',
		test: (p, name) => name.endsWith('.avhdx'),
		why: 'rozdílový disk kontrolního bodu Hyper-V — maže se smazáním kontrolního bodu, ruční smazání poškodí virtuální stroj',
		tool: 'Správce technologie Hyper-V → Kontrolní body'
	},
	{
		level: 'data',
		test: (p, name) => name.endsWith('.vhdx') || name.endsWith('.vhd'),
		why: 'virtuální disk — je v něm celý obsah virtuálního stroje; zmenšuje se v jeho správci, smazáním přijdeš o všechno v něm'
	},
	{
		level: 'aplikace',
		vzory: [
			'**/dockerdesktopwsl',
			'%programdata%/docker/windowsfilter',
			'%programdata%/dockerdesktop'
		],
		why: 'data Dockeru — místo uvolní docker system prune, smazání vezme i všechny obrazy a kontejnery',
		tool: 'docker system prune'
	},
	{
		level: 'aplikace',
		test: (p, name) => name.endsWith('.edb') || name === 'edb.log',
		why: 'databáze programu nebo služby (formát ESE) — drží ji otevřenou ten, kdo ji vytvořil'
	},
	{
		level: 'aplikace',
		test: (p, name) => name.endsWith('.ost'),
		why: 'offline kopie poštovní schránky Outlooku — Outlook si ji vytvoří znovu, odstraňuje se v nastavení účtu',
		tool: 'Outlook → Nastavení účtu'
	},
	{
		level: 'data',
		test: (p, name) => name.endsWith('.pst'),
		why: 'pošta uložená jen v tomhle počítači — často jediná kopie, smazáním o ni přijdeš'
	},
	// ── Program Files ──
	{
		level: 'mandatory',
		vzory: [
			'%programfiles%/{windows nt,windows mail,windows media player,windows photo viewer,windows portable devices,windows security,windows sidebar,windowspowershell,internet explorer,modifiablewindowsapps,uninstall information}'
		],
		why: 'součást Windows nainstalovaná v Program Files — odebírá se jen ve Volitelných funkcích Windows'
	},
	{
		level: 'aplikace',
		vzory: ['%programfiles%/microsoft/{edgewebview,edgecore,edgeupdate}'],
		why: 'Microsoft Edge WebView2 — běží na něm i Winsent a další programy; aktualizuje se sám'
	},
	{
		level: 'aplikace',
		vzory: ['%programfiles%/winsent'],
		why: 'instalace Winsentu — odebírá se odinstalací v Nastavení → Aplikace'
	},
	{
		level: 'kontejner',
		vzory: ['%programfiles%'],
		jenSlozka: true,
		why: 'Nainstalované programy. Odebírají se odinstalací v Nastavení → Aplikace, ne smazáním složky. Po smazání zůstanou rozbité zápisy v systému.',
		tool: 'Nastavení → Aplikace'
	},
	{
		level: 'kontejner',
		vzory: ['%programfiles%/common files'],
		jenSlozka: true,
		why: 'Sdílené součásti nainstalovaných programů. Odeberou se s programy, které je nainstalovaly, ne smazáním složky.',
		tool: 'Nastavení → Aplikace'
	},
	{
		level: 'aplikace',
		vzory: ['%programfiles%'],
		why: 'patří nainstalovanému programu — odebírá se odinstalací, ne ručním smazáním',
		tool: 'Programy → Odinstalovat'
	},
	{
		level: 'aplikace',
		vzory: ['%disk%/inetpub'],
		why: 'webový server IIS (volitelná funkce Windows) — odebírá se vypnutím funkce',
		tool: 'Ovládací panely → Programy → Zapnout nebo vypnout funkce systému Windows'
	},
	// ── ProgramData a profily ──
	{
		level: 'kontejner',
		vzory: ['%programdata%'],
		jenSlozka: true,
		why: 'Sdílená data programů a Windows pro všechny uživatele (licence, databáze, ovladače). Jako celek se nemaže.'
	},
	{
		level: 'kontejner',
		vzory: ['%disk%/users', '%disk%/documents and settings', '%disk%/users/all users'],
		jenSlozka: true,
		why: 'Složka uživatelských profilů: dokumenty, plocha, nastavení a data všech programů. Je největší, protože obsahuje skoro všechno tvoje. Celá se nikdy nemaže; profil jiného uživatele se odebírá v Nastavení → Účty.',
		tool: 'Nastavení → Účty; Vlastnosti systému → Profily uživatelů'
	},
	{
		level: 'kontejner',
		vzory: ['%disk%/users/default'],
		jenSlozka: true,
		why: 'Šablona profilu: Windows z ní zakládá každý nový účet. Nemazat.'
	},
	{
		level: 'mandatory',
		vzory: ['%disk%/users/default'],
		why: 'šablona profilu — Windows z ní zakládá každý nový účet; po zásahu můžou mít nové účty rozbité nastavení'
	},
	{
		level: 'kontejner',
		vzory: ['%disk%/users/public'],
		jenSlozka: true,
		why: 'Sdílené soubory všech uživatelů tohoto počítače. Celá se nemaže — rozklikni a hledej konkrétní věci.'
	},
	{
		level: 'data',
		vzory: ['%disk%/users/public'],
		why: 'sdílený soubor všech uživatelů tohoto počítače — smazáním zmizí všem'
	},
	{
		level: 'kontejner',
		vzory: ['%disk%/users/defaultuser0'],
		jenSlozka: true,
		why: 'Profil, který zůstal po instalaci Windows. Odebírá se ve Vlastnostech systému → Profily uživatelů, ne smazáním složky.',
		tool: 'Vlastnosti systému → Upřesnit → Profily uživatelů'
	},
	{
		level: 'kontejner',
		vzory: ['%userprofile%'],
		jenSlozka: true,
		why: 'Profil uživatele: plocha, dokumenty, nastavení a data programů. Velikost je součet obsahu — rozklikni a hledej konkrétní věci. Profil jiného uživatele se odebírá v Nastavení → Účty, ne smazáním složky.',
		tool: 'Nastavení → Účty'
	},
	{
		level: 'kontejner',
		vzory: ['%userprofile%/appdata', '%localappdata%', '%appdata%', '%locallow%'],
		jenSlozka: true,
		why: 'Data a nastavení všech tvých programů: přihlášení, hesla prohlížečů, uložené hry, pošta. Je velká, protože v ní jsou i mezipaměti. Smazat jde jen konkrétní mezipaměť uvnitř, nikdy celá složka.'
	},
	{
		level: 'kontejner',
		vzory: ['%localappdata%/packages'],
		jenSlozka: true,
		why: 'Data aplikací z Microsoft Storu. Každá aplikace má svou složku a smaže se s ní při odinstalaci.',
		tool: 'Nastavení → Aplikace'
	},
	{
		level: 'aplikace',
		vzory: ['%localappdata%/packages/*'],
		why: 'data aplikace z Microsoft Storu — smažou se při odinstalaci nebo tlačítkem Obnovit v nastavení aplikace',
		tool: 'Nastavení → Aplikace → (aplikace) → Upřesnit → Obnovit'
	},
	{
		level: 'kontejner',
		vzory: ['%localappdata%/programs'],
		jenSlozka: true,
		why: 'Programy nainstalované jen pro tvůj účet. Odebírají se odinstalací, ne smazáním složky.',
		tool: 'Nastavení → Aplikace'
	},
	{
		level: 'aplikace',
		vzory: ['%localappdata%/programs/*'],
		why: 'program nainstalovaný jen pro tvůj účet — odebírá se odinstalací, ne ručním smazáním',
		tool: 'Programy → Odinstalovat'
	},
	{
		level: 'data',
		vzory: ['%userprofile%/{onedrive,onedrive - *}'],
		why: 'tvoje soubory synchronizované s OneDrivem — smazání se promítne do cloudu a na další zařízení'
	},
	{
		level: 'data',
		vzory: ['%userprofile%/downloads'],
		why: 'tvoje stažené soubory — Windows je samo nemaže, pokud si to nezapneš v Inteligentním úložišti; smazáním o ně přijdeš'
	},
	{
		level: 'data',
		vzory: ['%userprofile%/{desktop,documents,pictures,videos,music,saved games}'],
		why: 'tvoje soubory — Windows je nepotřebuje, ale smazáním o ně přijdeš'
	},
	{
		// Tečkové položky v kořeni profilu (.gitconfig, .cargo, .vscode)
		// jsou nastavení a data vývojářských nástrojů, ne dokumenty.
		// Ostatní soubory přímo v profilu zůstávají bez značky — mazání
		// tam stejně odmítne služba a řekne proč.
		level: 'aplikace',
		test: (p) => /^[a-z]:\\users\\[^\\]+\\\.[^\\]+$/.test(p),
		why: 'nastavení nebo data programu přímo v profilu (třeba .gitconfig, .cargo) — patří nástroji, který je vytvořil'
	}
];

const zkompiluj = (pravidla) =>
	pravidla.map((r) => ({ ...r, re: (r.vzory ?? []).map((v) => vzor(v, r.jenSlozka)) }));
const PRAVIDLA = [...zkompiluj(PRAVIDLA_WINDOWS), ...PATH_KB, ...zkompiluj(PRAVIDLA_OBECNA)];

function normalizuj(path) {
	return (path ?? '')
		.replace(/\//g, '\\')
		.toLowerCase()
		.replace(/^\\\\\?\\/, '')
		.replace(/\\+$/, '');
}

function vysledek(r) {
	const out = { level: r.level, reason: r.why };
	if (r.tool) out.tool = r.tool;
	if (r.condition) out.condition = r.condition;
	if (r.certainty) out.certainty = r.certainty;
	if (r.hledat) out.hledat = r.hledat;
	return out;
}

/// Program Files služba nemaže NIKDY (validate::protected_path —
/// programy se tam odebírají odinstalací). Steam ale ve výchozím
/// umístění leží celý v C:\Program Files (x86)\Steam i s mezipaměťmi
/// a uloženými daty, a znalostní báze je pozná dřív než obecné pravidlo
/// pro Program Files. Zelená značka s košem by pak slibovala něco, co
/// služba vzápětí odmítne. Důvod a správná cesta z báze zůstanou, jen
/// úroveň je „patří programu" (bez koše).
const RE_PROGRAM_FILES = vzor('%programfiles%');
const V_PROGRAM_FILES = 'Leží v Program Files, kde Winsent nic nemaže — místo uvolni v programu, kterému patří.';

function mimoProgramFiles(p, out) {
	if (!smiDoKose(out) || !RE_PROGRAM_FILES.test(p)) return out;
	return {
		...out,
		level: 'aplikace',
		reason: `${out.reason.replace(/[.\s]+$/, '')}. ${V_PROGRAM_FILES}`
	};
}

/// Co je tahle cesta zač? `null` = Winsent neví, jinak
/// `{ level, reason, tool?, condition?, certainty?, hledat? }`:
///   level     — viz úrovně výš,
///   reason    — česky celou větou, jde rovnou do UI,
///   tool      — správná cesta, jak místo uvolnit (nástroj Windows nebo
///               programu),
///   condition — co udělat předtím („zavři hry"),
///   certainty — 'high' (doloženo výrobcem) / 'medium' (jen sekundární
///               zdroje); jen u záznamů ze znalostní báze,
///   hledat    — co hledat na webu místo jména souboru (hashe typu
///               „a3f9….nvph" nic neřeknou).
export function systemPathInfo(path = '') {
	const p = normalizuj(path);
	if (!p) return null;
	const name = p.slice(p.lastIndexOf('\\') + 1);
	// Kořen svazku má přednost: pagefile a hiberfil bývají úplně
	// největší soubory na disku a zaslouží si přesnou hlášku.
	// Jen vlastní klíče tabulky: `C:\constructor` či `C:\__proto__` by
	// jinak přes Object.prototype dostaly značku povinné součásti
	// s tooltipem „function Object() { [native code] }" a skrytý koš.
	if (/^[a-z]:\\[^\\]+$/.test(p) && Object.hasOwn(ROOT_SYSTEM_FILES, name)) {
		return vysledek({ level: 'mandatory', ...ROOT_SYSTEM_FILES[name] });
	}
	for (const r of PRAVIDLA) {
		if (r.re.some((re) => re.test(p)) || r.test?.(p, name)) return mimoProgramFiles(p, vysledek(r));
	}
	return null;
}

/// Smí položka do koše (tlačítko v řádku i položka v kontextovém menu)?
/// Jen to, co mazat jde: mezipaměť, tvoje data (dialog varuje) a to,
/// co Winsent nezná (dialog varuje taky). Systém, kontejnery a věci
/// programů ne — jejich důvod říká, jak se uklízejí správně.
export function smiDoKose(info) {
	return !info || info.level === 'data' || info.level === 'cache';
}

/// Krátké štítky úrovní (plný badge, podtitulek menu, nápověda u
/// zakázané akce).
export const STITKY = {
	mandatory: 'součást Windows',
	managed: 'spravuje Windows',
	kontejner: 'nemazat celé',
	aplikace: 'patří programu',
	data: 'tvoje data',
	cache: 'mezipaměť'
};

/// Jednou větou, co úroveň pro uživatele znamená. U mezipaměti podle
/// jistoty: „smazat jde" smí říct jen to, co doložil výrobce.
/// `maNastroj` platí jen pro 'managed': Prefetch, CrashDumps, WebCache
/// ani LiveKernelReports žádný nástroj Windows na úklid nemají, a věta
/// „místo uvolní jeho nástroj" by tooltipu odporovala hned o řádek níž.
export function souhrnUrovne(level, certainty = null, maNastroj = true) {
	switch (level) {
		case 'mandatory':
			return 'součást Windows — nemazat';
		case 'managed':
			return maNastroj
				? 'spravuje Windows — místo uvolní jeho nástroj, ne ruční smazání'
				: 'spravuje Windows — ručně nemazat';
		case 'kontejner':
			return 'nemazat celé — velikost je součet obsahu, rozklikni';
		case 'aplikace':
			return 'patří programu — nemazat ručně';
		case 'data':
			return 'tvoje data — smazat jde, ale přijdeš o ně';
		case 'cache':
			return certainty === 'high'
				? 'mezipaměť — smazat jde, vytvoří se znovu'
				: 'obvykle bezpečné — výrobce to oficiálně nepotvrzuje';
		default:
			return 'Winsent neví, co to je — nemaž, dokud si nejsi jistý';
	}
}

/// Celý tooltip ke značce: co to znamená, proč, předtím, správná cesta.
export function titulek(info) {
	if (!info) return souhrnUrovne(null);
	return [
		souhrnUrovne(info.level, info.certainty, !!info.tool),
		info.reason,
		info.condition ? `Předtím: ${info.condition}` : '',
		info.tool ? `Správná cesta: ${info.tool}` : ''
	]
		.filter(Boolean)
		.join('\n');
}

/// Kopie v duplicitách. Stejný soubor v datech dvou programů (nebo dvou
/// profilů téhož launcheru) není zbytečný — každou kopii používá někdo
/// jiný. Na stroji vlastníka to byly skoro všechny největší skupiny
/// (profily Modrinthu).
export function infoKopie(path = '') {
	const info = systemPathInfo(path);
	if (info) return info;
	const p = normalizuj(path);
	if (/^[a-z]:\\users\\[^\\]+\\appdata\\/.test(p) || /^[a-z]:\\programdata\\/.test(p)) {
		return {
			level: 'aplikace',
			reason: 'kopie uvnitř dat programu — obvykle ji používá jiný profil nebo jiný program, takže zbytečná není; ruční smazání ho může rozbít'
		};
	}
	return null;
}

/// Prázdné soubory, které jsou značkou: prázdné, ale potřebné.
const ZNACKY = /^(__init__\.py|py\.typed|\.gitkeep|\.keep|\.nomedia|\.placeholder|lock|.+\.lock|.+\.lck)$/;

/// Soubor ze seznamu 0 B. Jméno samo neřekne, jestli je prázdný soubor
/// smetí — některé jsou značky, podle kterých program nebo nástroj
/// něco pozná (balíček Pythonu, prázdná složka v gitu, zámek běžícího
/// programu).
export function infoPrazdneho(path = '') {
	const info = systemPathInfo(path);
	if (info) return info;
	const p = normalizuj(path);
	const name = p.slice(p.lastIndexOf('\\') + 1);
	if (ZNACKY.test(name)) {
		return {
			level: 'aplikace',
			reason: 'prázdný, ale potřebný — značka, podle které program nebo nástroj něco pozná (balíček Pythonu, prázdná složka v gitu, zámek běžícího programu)'
		};
	}
	return null;
}

/// Mapa souborů v Programech: služba dává roli podle jména („cokoli
/// s cache v názvu je Cache"), a tak byla Package Cache — bez které
/// program nejde odinstalovat — označená jako mezipaměť. Roli `cache`
/// proto potvrzuje sdílená klasifikace; co znalostní báze nezná, je
/// neutrální `data`.
/// Vrací `{ role, info }`: `role` je klíč role ze služby (install, data,
/// config, cache, logs, registry), `info` výsledek `systemPathInfo`
/// (nebo null) pro značku a tooltip.
export function roleProSoubor(path = '', roleSluzby = 'data') {
	if (roleSluzby === 'registry') return { role: 'registry', info: null };
	const info = systemPathInfo(path);
	const role = roleSluzby === 'cache' && info?.level !== 'cache' ? 'data' : roleSluzby;
	return { role, info };
}
