// Znalostní báze cest: mezipaměti a data programů (ne Windows).
//
// Proč zvlášť od mandatory.js: tam je stavba Windows (co je systém,
// co spravuje jeho nástroj). Tady jsou cizí programy — ovladače,
// prohlížeče, launchery, vývojářské nástroje — a hlavně JEDINÉ místo,
// odkud smí přijít úroveň `cache`, tedy „tohle smazat jde".
//
// Pravidlo jistoty (přání vlastníka: o mezipaměti mluvit jen se 100%
// jistotou):
//   certainty 'high'   — výrobce to sám dokumentuje (odkaz u záznamu),
//   certainty 'medium' — jen sekundární zdroje (fóra, návody) nebo
//                        výrobce dokumentuje nástroj, ale ne cestu.
// UI podle toho mluví: 'high' = „mezipaměť — smazat jde, vytvoří se
// znovu", 'medium' = „obvykle bezpečné — výrobce to oficiálně
// nepotvrzuje". Nic, co tu není, nedostane pozitivní štítek.
//
// `cache` (= koš povolený) jen pro verdikty SAFE / SAFE_CLOSE_APP.
// Verdikt USE_TOOL („smazat jen nástrojem") je `aplikace` s nástrojem
// jako správnou cestou, i když je doložený výrobcem — výrobce tam
// dokumentuje nástroj, ne ruční mazání souborů.
//
// Zdroj textů a jistot: průzkum 2026-10-11 (cache-kb.md). U každého
// záznamu je odkaz, o který se jistota opírá — při úpravě textu nebo
// cesty ho zkontroluj, ať tu nezůstane „high" bez dokladu.

// ── Vzory cest ───────────────────────────────────────────────────────
// Píšou se s lomítky „/" a malými písmeny, protože zdvojená zpětná
// lomítka v regexech se nedají číst ani kontrolovat proti dokumentaci.
//   %localappdata% %appdata% %locallow% %userprofile% — profil
//                  libovolného uživatele (C:\Users\<kdokoli>\…),
//   %programdata% %programfiles% (obě, i x86) %disk% (kterýkoli svazek)
//   %systemdrive% %systemroot% — systémový disk / složka Windows,
//   *        jedna celá složka libovolného jména (uvnitř jména: cokoli),
//   {a,b}    jedna z možností,
//   ** na začátku — kdekoli, ale vždy od hranice složky.
// Vzor sedí na složku SAMOTNOU i na všechno pod ní. Koncové lomítko
// by složku samotnou minulo (cesty chodí bez něj) — přesně to dřív
// dělalo z C:\Windows\Temp „součást Windows".

/// Systémový disk. Webové UI %SystemRoot% nezná a služba ho neposílá;
/// výchozí instalace Windows je na C: (i na tomhle stroji). `D:\Windows`
/// proto mandatory.js označí jako jinou instalaci, ne jako běžící systém.
export const SYSTEMOVY_DISK = 'c';

const SLOZKA = '[^\\\\]+';
const PROFIL = `[a-z]:\\\\users\\\\${SLOZKA}`;
const KORENY = {
	'%userprofile%': PROFIL,
	'%localappdata%': `${PROFIL}\\\\appdata\\\\local`,
	'%appdata%': `${PROFIL}\\\\appdata\\\\roaming`,
	'%locallow%': `${PROFIL}\\\\appdata\\\\locallow`,
	'%programdata%': '[a-z]:\\\\programdata',
	'%programfiles%': '[a-z]:\\\\program files(?: \\(x86\\))?',
	'%disk%': '[a-z]:',
	'%systemdrive%': `${SYSTEMOVY_DISK}:`,
	'%systemroot%': `${SYSTEMOVY_DISK}:\\\\windows`
};

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const glob = (s) => s.split('*').map(escape).join('[^\\\\]*');

function segment(s) {
	if (s === '*') return SLOZKA;
	return s
		.split(/(\{[^}]*\})/)
		.map((k) =>
			k.startsWith('{') ? `(?:${k.slice(1, -1).split(',').map(glob).join('|')})` : glob(k)
		)
		.join('');
}

/// Vzor → regex. `jenSlozka` = jen ta složka sama, ne obsah (kořeny,
/// které jsou „kontejner", zatímco jejich obsah je něco jiného).
export function vzor(s, jenSlozka = false) {
	const casti = s.toLowerCase().split('/');
	let hlava = '^';
	if (casti[0] === '**') {
		hlava = '(?:^|\\\\)';
		casti.shift();
	} else if (Object.hasOwn(KORENY, casti[0])) {
		hlava = '^' + KORENY[casti.shift()] + (casti.length ? '\\\\' : '');
	}
	const telo = casti.map(segment).join('\\\\');
	return new RegExp(hlava + telo + (jenSlozka ? '$' : '(?:\\\\|$)'));
}

// ── Opakované texty ──────────────────────────────────────────────────
const ZAVRI_HRY = 'zavři hry a programy, které kreslí přes grafickou kartu';
const NVIDIA_NASTROJ =
	'aplikace NVIDIA → Graphics → Global Settings → Shader Cache Size: Off, restart, vyprázdnit, Driver Default, restart (postup NVIDIA KB 5735)';
const NVIDIA_DXCACHE =
	'Mezipaměť shaderů, které ovladač NVIDIA přeložil pro hry v DirectX. Smazat ji jde a ovladač ji vytvoří znovu, jen první spuštění her bude pomalejší a může se trhat. Velikost hlídá nastavení „Velikost mezipaměti shaderů" v aplikaci NVIDIA, takže se zase zaplní.';
const PROHLIZECE = ['google/chrome', 'microsoft/edge', 'bravesoftware/brave-browser'];

/// Záznamy se zkoušejí shora dolů, vyhrává první shoda — konkrétní nad
/// obecnými (Discord nad obecným Electronem, Docker nad ext4.vhdx).
const ZAZNAMY = [
	// ── A. Grafické karty a shadery ──────────────────────────────────
	{
		id: 'nvidia-dxcache',
		vzory: ['%localappdata%/nvidia/dxcache'],
		level: 'cache',
		// https://nvidia.custhelp.com/app/answers/detail/a_id/5735
		certainty: 'high',
		why: NVIDIA_DXCACHE,
		condition: ZAVRI_HRY,
		tool: NVIDIA_NASTROJ,
		hledat: 'NVIDIA DXCache'
	},
	{
		// Starší ovladače ukládaly totéž do LocalLow — KB 5735 tuhle
		// cestu nezmiňuje, proto jen 'medium'.
		id: 'nvidia-dxcache-locallow',
		vzory: ['%locallow%/nvidia/perdriverversion/dxcache', '%locallow%/nvidia/dxcache'],
		level: 'cache',
		certainty: 'medium',
		why: NVIDIA_DXCACHE,
		condition: ZAVRI_HRY,
		tool: NVIDIA_NASTROJ,
		hledat: 'NVIDIA DXCache'
	},
	{
		id: 'nvidia-glcache',
		vzory: ['%localappdata%/nvidia/glcache'],
		level: 'cache',
		// https://nvidia.custhelp.com/app/answers/detail/a_id/5735
		certainty: 'high',
		why: 'Mezipaměť shaderů ovladače NVIDIA pro hry a programy v OpenGL a Vulkanu. Ovladač ji po smazání vytvoří znovu, jen první spuštění bude pomalejší.',
		condition: ZAVRI_HRY,
		tool: NVIDIA_NASTROJ,
		hledat: 'NVIDIA GLCache'
	},
	{
		id: 'nvidia-nvcache-user',
		vzory: ['%localappdata%/nvidia corporation/nv_cache'],
		level: 'cache',
		// https://nvidia.custhelp.com/app/answers/detail/a_id/5735
		certainty: 'high',
		why: 'Starší umístění mezipaměti ovladače NVIDIA. Ovladač si ji vytvoří znovu.',
		condition: ZAVRI_HRY,
		tool: NVIDIA_NASTROJ,
		hledat: 'NVIDIA NV_Cache'
	},
	{
		id: 'nvidia-nvcache-programdata',
		vzory: ['%programdata%/nvidia corporation/nv_cache'],
		level: 'cache',
		certainty: 'medium',
		why: 'Sdílená mezipaměť ovladače NVIDIA ze starších verzí. Ovladač si ji vytvoří znovu.',
		condition: ZAVRI_HRY,
		hledat: 'NVIDIA NV_Cache'
	},
	{
		id: 'nvidia-computecache',
		vzory: ['%appdata%/nvidia/computecache'],
		level: 'cache',
		// https://docs.nvidia.com/cuda/cuda-programming-guide/05-appendices/environment-variables.html
		// https://developer.nvidia.com/blog/cuda-pro-tip-understand-fat-binaries-jit-caching/
		certainty: 'high',
		why: 'Mezipaměť CUDA: přeložené výpočetní programy pro grafickou kartu (AI nástroje, renderery). Má omezenou velikost (1 GiB), po aktualizaci ovladače se stejně zahodí a při dalším spuštění se vytvoří znovu.',
		condition: 'zavři programy, které počítají na grafické kartě (AI nástroje, renderery)',
		tool: 'proměnné prostředí CUDA_CACHE_MAXSIZE / CUDA_CACHE_DISABLE',
		hledat: 'NVIDIA ComputeCache CUDA'
	},
	{
		id: 'nvidia-optixcache',
		vzory: ['%localappdata%/nvidia/optixcache'],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměť pro ray tracing (OptiX) v renderovacích programech. Vytvoří se znovu, první render bude pomalejší.',
		condition: 'zavři renderovací programy',
		hledat: 'NVIDIA OptixCache'
	},
	{
		id: 'nvidia-driver-extract',
		vzory: ['%systemdrive%/nvidia/displaydriver'],
		level: 'cache',
		certainty: 'medium',
		why: 'Rozbalený instalátor ovladače NVIDIA. Po úspěšné instalaci už není potřeba; přijdeš jen o možnost přeinstalovat tuto verzi bez nového stažení.',
		hledat: 'C:\\NVIDIA DisplayDriver složka'
	},
	{
		id: 'nvidia-downloader',
		vzory: ['%programdata%/nvidia corporation/downloader'],
		level: 'cache',
		certainty: 'medium',
		why: 'Stažené instalační balíčky ovladačů z aplikace NVIDIA / GeForce Experience. Nainstalovaný ovladač je nepotřebuje, ale nemaž je během probíhající aktualizace. NVIDIA to oficiálně nepotvrzuje.',
		condition: 'neběží aktualizace ovladače NVIDIA',
		hledat: 'NVIDIA Corporation Downloader složka'
	},
	{
		id: 'nvidia-app-ota',
		vzory: ['%programdata%/nvidia corporation/nvidia app/updateframework/ota-artifacts'],
		level: 'cache',
		certainty: 'medium',
		why: 'Stažené aktualizace ovladače a aplikace NVIDIA. Po instalaci se podle uživatelů nepotřebují, NVIDIA to ale nedokumentuje. Nemazat během aktualizace.',
		condition: 'neběží aktualizace ovladače ani aplikace NVIDIA',
		hledat: 'NVIDIA app ota-artifacts'
	},
	{
		id: 'nvidia-installer2',
		vzory: ['%programfiles%/nvidia corporation/installer2'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Instalační data softwaru NVIDIA. Návody je mažou, jenže pak nemusí jít software NVIDIA odinstalovat. Výrobce to nedokumentuje, takže raději nechat.',
		tool: 'odinstalace v Nastavení → Aplikace',
		hledat: 'NVIDIA Installer2'
	},
	{
		id: 'nvidia-ngx-models',
		vzory: ['%programdata%/nvidia/ngx/models'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Modely DLSS, které stahuje a aktualizuje ovladač NVIDIA. Ručně se mažou jen při pádech hry s DLSS, ovladač je pak stáhne znovu; jinak je nech být.',
		tool: 'spravuje ovladač NVIDIA',
		hledat: 'NVIDIA NGX models DLSS'
	},
	{
		// Mimo průzkum mezipamětí — z auditu objektů (2026-10-11).
		id: 'nvidia-drs',
		vzory: ['%programdata%/nvidia corporation/drs'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Nastavení ovladače NVIDIA: 3D nastavení a profily her z Ovládacího panelu NVIDIA. Smazáním se všechno vrátí na výchozí hodnoty.',
		hledat: 'NVIDIA Drs nastavení ovladače'
	},
	{
		id: 'nvidia-app-cef',
		vzory: ['%localappdata%/nvidia corporation/nvidia app/cefcache'],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměť vestavěného prohlížeče v aplikaci NVIDIA. Po zavření aplikace jde smazat a vytvoří se znovu.',
		condition: 'zavři aplikaci NVIDIA'
	},
	{
		// Verdikt USE_TOOL: AMD dokumentuje tlačítko, ne mazání složek →
		// `aplikace` (bez koše), nástroj je správná cesta.
		id: 'amd-shader-cache',
		vzory: ['%localappdata%/amd/{dxcache,dxccache,glcache,vkcache}'],
		level: 'aplikace',
		// Nástroj: https://www.amd.com/en/resources/support-articles/faqs/dh3-012.html
		// Názvy složek ale AMD nedokumentuje → 'medium'.
		certainty: 'medium',
		why: 'Mezipaměť shaderů ovladače AMD. Vymaže ji tlačítko „Obnovit mezipaměť shaderů" v AMD Software; hry pak poprvé načítají déle.',
		condition: ZAVRI_HRY,
		tool: 'AMD Software: Adrenalin → Gaming → Graphics → Advanced → Reset Shader Cache'
	},
	{
		id: 'amd-installer-extract',
		vzory: ['%systemdrive%/amd'],
		level: 'cache',
		certainty: 'medium',
		why: 'Rozbalené instalátory ovladačů AMD. Po instalaci a restartu nejsou potřeba. Pokud máš ovladače čipové sady AMD, nech složku být, může v ní být jejich odinstalátor.',
		hledat: 'C:\\AMD složka instalátor'
	},
	{
		id: 'intel-shader-cache',
		vzory: ['%localappdata%/intel/shadercache', '%locallow%/intel/shadercache'],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměť shaderů ovladače Intel. Vytvoří se znovu, první spuštění her bude pomalejší.',
		condition: ZAVRI_HRY
	},
	{
		id: 'iconcache',
		vzory: ['%localappdata%/iconcache.db', '%localappdata%/microsoft/windows/explorer/iconcache_*.db'],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměť ikon Průzkumníku. Drží ji otevřenou Průzkumník a sama se obnoví; místo nezabírá.'
	},

	// ── WSL a Docker: celé disky s daty ─────────────────────────────
	{
		id: 'docker-vhdx',
		vzory: [
			'%localappdata%/docker/wsl/disk/docker_data.vhdx',
			'%localappdata%/docker/wsl/data/ext4.vhdx'
		],
		level: 'data',
		certainty: 'medium',
		why: 'Všechny obrazy, kontejnery a svazky Dockeru. Místo se uvolňuje v Docker Desktop nebo příkazem docker system prune.',
		tool: 'Docker Desktop → Troubleshoot, nebo docker system prune'
	},
	{
		id: 'wsl-vhdx',
		// Kdekoli: distribuce může mít vlastní umístění (BasePath).
		vzory: ['**/ext4.vhdx'],
		level: 'data',
		// https://learn.microsoft.com/en-us/windows/wsl/disk-space
		certainty: 'high',
		why: 'Celý disk linuxové distribuce ve WSL se vším, co v ní máš. Smazat jen příkazem wsl --unregister, zmenšit přes správu WSL.',
		tool: 'wsl --unregister <distribuce>, wsl --manage'
	},

	// ── D. Prohlížeče a aplikace na Electronu ───────────────────────
	{
		id: 'chromium-cache',
		vzory: PROHLIZECE.flatMap((b) => [
			`%localappdata%/${b}/user data/*/{cache,code cache,gpucache}`,
			`%localappdata%/${b}/user data/{grshadercache,shadercache,graphitedawncache}`
		]),
		level: 'cache',
		// Nástroj: https://support.google.com/accounts/answer/32050,
		// umístění: https://chromium.googlesource.com/chromium/src/+/HEAD/docs/user_data_dir.md
		// Jména podsložek ale dokumentovaná nejsou → 'medium'.
		certainty: 'medium',
		why: 'Mezipaměť prohlížeče: obrázky stránek, předpřipravené skripty a shadery. Po smazání se stránky poprvé načtou pomaleji; přihlášení a hesla zůstanou. Nejlépe ji smažeš přímo v prohlížeči.',
		condition: 'zavři prohlížeč',
		tool: 'v prohlížeči: Smazat údaje o prohlížení → Obrázky a soubory v mezipaměti'
	},
	{
		id: 'chromium-profile',
		vzory: PROHLIZECE.map((b) => `%localappdata%/${b}/user data`),
		level: 'data',
		// https://chromium.googlesource.com/chromium/src/+/HEAD/docs/user_data_dir.md
		certainty: 'high',
		why: 'Tvůj profil prohlížeče: hesla, záložky, historie, přihlášení a rozšíření. Smazáním o ně přijdeš (pokud nejsou synchronizované).'
	},
	{
		id: 'firefox-cache',
		vzory: ['%localappdata%/mozilla/firefox/profiles/*/{cache2,startupcache,thumbnails}'],
		level: 'cache',
		// https://firefox-source-docs.mozilla.org/toolkit/profile/index.html
		certainty: 'high',
		why: 'Mezipaměť Firefoxu, oddělená od profilu. Po zavření Firefoxu jde smazat; hesla a záložky v ní nejsou.',
		condition: 'zavři Firefox',
		tool: 'Firefox → Nastavení → Soukromí → Vymazat data → Webový obsah v mezipaměti'
	},
	{
		id: 'firefox-profile',
		vzory: ['%appdata%/mozilla/firefox'],
		level: 'data',
		// https://support.mozilla.org/kb/profiles-where-firefox-stores-user-data
		certainty: 'high',
		why: 'Tvůj profil Firefoxu: záložky, hesla, historie, rozšíření. Nemazat.'
	},
	{
		id: 'teams-cache',
		vzory: [
			'%localappdata%/packages/msteams_8wekyb3d8bbwe/localcache/microsoft/msteams',
			'%appdata%/microsoft/teams'
		],
		level: 'cache',
		// https://learn.microsoft.com/en-us/troubleshoot/microsoftteams/teams-administration/clear-teams-cache
		certainty: 'high',
		why: 'Mezipaměť Teams. Po ukončení Teams jde smazat a další spuštění trvá déle. Obnovení aplikace v Nastavení smaže i osobní nastavení.',
		condition: 'ukonči Teams',
		tool: 'Nastavení → Aplikace → Microsoft Teams → Upřesnit → Obnovit'
	},
	{
		id: 'spotify-cache',
		vzory: [
			'%localappdata%/spotify/{storage,data}',
			'%localappdata%/packages/spotifyab.spotifymusic_zpdnekdrzrea0/localcache/spotify/data'
		],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměť Spotify. Po zavření aplikace jde smazat; skladby stažené offline se pak musí stáhnout znovu.',
		condition: 'zavři Spotify',
		tool: 'Spotify → Nastavení → Úložiště'
	},
	{
		id: 'discord-cache',
		vzory: ['%appdata%/discord/{cache,code cache,gpucache}'],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměť Discordu. Po úplném zavření Discordu (i z oznamovací oblasti) jde smazat; účet ani zprávy v ní nejsou.',
		condition: 'úplně zavři Discord, i z oznamovací oblasti'
	},
	{
		// Jméno složky samo nezaručí, že jde o Electron — proto 'medium'.
		id: 'electron-cache',
		vzory: [
			'%appdata%/*/{cache,code cache,gpucache,dawncache,dawngraphitecache,dawnwebgpucache}'
		],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměť vestavěného prohlížeče aplikace (Discord, VS Code, Slack…). Po zavření aplikace jde smazat a vytvoří se znovu.',
		condition: 'zavři aplikaci, které složka patří'
	},
	{
		id: 'electron-session',
		vzory: ['%appdata%/*/{local storage,indexeddb,session storage,cookies,network}'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Přihlášení a místní data aplikace. Smazáním se odhlásíš a můžeš přijít o neodeslaná data.'
	},

	// ── E. Herní launchery ──────────────────────────────────────────
	{
		id: 'steam-shadercache',
		vzory: ['**/steamapps/shadercache'],
		level: 'cache',
		certainty: 'medium',
		why: 'Předkompilované shadery her ze Steamu. Po smazání je Steam stáhne nebo vytvoří znovu, hra se může zpočátku trhat.',
		condition: 'zavři Steam a hry',
		tool: 'Steam → Nastavení → Předběžné ukládání shaderů'
	},
	{
		id: 'steam-download-cache',
		vzory: ['**/steam/depotcache', '**/steamapps/downloading'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Rozpracovaná stahování Steamu a jejich evidence. Smaže je tlačítko „Vymazat mezipaměť stahování"; rozpracovaná stahování začnou znovu.',
		tool: 'Steam → Nastavení → Stahování → Vymazat mezipaměť stahování'
	},
	{
		id: 'steam-htmlcache',
		vzory: ['%localappdata%/steam/htmlcache', '**/steam/config/htmlcache'],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměť obchodu a knihovny ve Steamu (webový obsah). Po zavření Steamu jde smazat.',
		condition: 'zavři Steam'
	},
	{
		id: 'steam-appcache',
		vzory: ['**/steam/appcache'],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměť informací o hrách ve Steamu. Po zavření Steamu jde smazat a Steam si ji stáhne znovu.',
		condition: 'zavři Steam'
	},
	{
		id: 'steam-library',
		vzory: ['**/steamapps', '**/steamapps/common', '%disk%/steamlibrary'],
		jenSlozka: true,
		level: 'kontejner',
		certainty: 'medium',
		why: 'Knihovna her ze Steamu. Velikost je součet nainstalovaných her — hry se odinstalují ve Steamu, ne smazáním složky.',
		tool: 'Steam → Knihovna → Spravovat → Odinstalovat'
	},
	{
		id: 'steam-games',
		vzory: ['**/steamapps/common/*', '**/steamapps/appmanifest_*.acf'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Nainstalovaná hra ze Steamu. Odebírá se odinstalací ve Steamu; po ručním smazání Steam hru „ztratí".',
		tool: 'Steam → Knihovna → Spravovat → Odinstalovat'
	},
	{
		id: 'steam-userdata',
		vzory: ['**/steam/userdata'],
		level: 'data',
		certainty: 'medium',
		why: 'Tvoje nastavení, snímky obrazovky a místní uložené pozice her ve Steamu. Nemazat.'
	},
	{
		id: 'epic-webcache',
		vzory: ['%localappdata%/epicgameslauncher/saved/{webcache,webcache_*}'],
		level: 'cache',
		// https://www.epicgames.com/help/en-US/c-Category_EpicGamesStore/c-EpicGamesStore_LauncherSupport/how-do-i-clear-the-launcher-cache-on-pc-a000086158
		certainty: 'high',
		why: 'Mezipaměť Epic Games Launcheru. Po jeho ukončení jde smazat; nainstalované hry zůstanou.',
		condition: 'ukonči Epic Games Launcher'
	},
	{
		id: 'epic-manifests',
		vzory: ['%programdata%/epic/epicgameslauncher/data/manifests'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Evidence nainstalovaných her v Epic Launcheru. Po smazání launcher hry „ztratí".'
	},
	{
		// Mimo průzkum mezipamětí — z auditu souborů (2026-10-11).
		id: 'herni-knihovny',
		vzory: ['%disk%/riot games', '%disk%/xboxgames'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Nainstalované hry (Riot Client, aplikace Xbox). Odinstalují se v jejich launcheru nebo v Nastavení → Aplikace, ne smazáním složky.',
		tool: 'launcher hry, nebo Nastavení → Aplikace'
	},
	{
		id: 'xbox-gamingroot',
		vzory: ['%disk%/.gamingroot'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Značka knihovny her aplikace Xbox — podle ní aplikace Xbox pozná, na kterém disku má hry.'
	},

	// ── F. Vývojářské nástroje ──────────────────────────────────────
	{
		id: 'npm-cache',
		vzory: ['%localappdata%/npm-cache', '%appdata%/npm-cache'],
		level: 'cache',
		// https://docs.npmjs.com/cli/v11/commands/npm-cache
		certainty: 'high',
		why: 'Mezipaměť balíčků npm. Smazat jde; npm ji opravuje sám a balíčky si stáhne znovu.',
		tool: 'npm cache clean --force'
	},
	{
		id: 'pip-cache',
		vzory: ['%localappdata%/pip/cache'],
		level: 'cache',
		// https://pip.pypa.io/en/stable/topics/caching/
		certainty: 'high',
		why: 'Mezipaměť balíčků Pythonu. Smazat jde, pip si je stáhne znovu.',
		tool: 'pip cache purge'
	},
	{
		id: 'cargo-registry',
		vzory: ['%userprofile%/.cargo/registry/{cache,src,index}', '%userprofile%/.cargo/git/{db,checkouts}'],
		level: 'cache',
		// https://doc.rust-lang.org/cargo/guide/cargo-home.html
		certainty: 'high',
		why: 'Stažené zdrojové kódy knihoven Rustu. Smazat jde, když neběží build; Cargo si je stáhne znovu (offline build pak nepůjde).',
		condition: 'neběží žádný build v Rustu'
	},
	{
		id: 'cargo-bin',
		vzory: ['%userprofile%/.cargo/bin'],
		level: 'aplikace',
		// https://doc.rust-lang.org/cargo/guide/cargo-home.html
		certainty: 'high',
		why: 'Nainstalované nástroje Rustu (cargo, rustc, programy z cargo install). Smazáním přestane Rust fungovat.',
		tool: 'cargo uninstall, rustup toolchain uninstall'
	},
	{
		id: 'rustup',
		vzory: ['%userprofile%/.rustup'],
		level: 'aplikace',
		certainty: 'medium',
		why: 'Nainstalované verze Rustu (toolchainy). Odebírají se přes rustup, ručním smazáním přestane Rust fungovat.',
		tool: 'rustup toolchain uninstall'
	},
	{
		// Verdikt USE_TOOL → `aplikace`, ne `cache`. NuGet bere balíček za
		// nainstalovaný, dokud leží jeho .nupkg.metadata: smazání jednoho
		// souboru uvnitř se samo neobnoví a build spadne, dokud se
		// nevyčistí celá složka nástrojem.
		id: 'nuget-packages',
		vzory: ['%userprofile%/.nuget/packages'],
		level: 'aplikace',
		// https://learn.microsoft.com/en-us/nuget/consume-packages/managing-the-global-packages-and-cache-folders
		certainty: 'high',
		why: 'Rozbalené balíčky NuGet pro všechny .NET projekty. Smazat jde nástrojem při zavřeném Visual Studiu; projekty si je při obnově stáhnou znovu.',
		condition: 'zavři Visual Studio a neběží žádný build',
		tool: 'dotnet nuget locals global-packages --clear'
	},
	{
		id: 'nuget-http-cache',
		vzory: [
			'%localappdata%/nuget/{v3-cache,plugins-cache}',
			'%localappdata%/temp/nugetscratch'
		],
		level: 'cache',
		// https://learn.microsoft.com/en-us/nuget/consume-packages/managing-the-global-packages-and-cache-folders
		certainty: 'high',
		why: 'Mezipaměť stahování NuGet. Smazat jde kdykoli.',
		tool: 'dotnet nuget locals all --clear'
	},
	{
		id: 'gradle-caches',
		vzory: ['%userprofile%/.gradle/{caches,daemon}', '%userprofile%/.gradle/wrapper/dists'],
		level: 'cache',
		certainty: 'medium',
		why: 'Mezipaměti Gradle. Gradle je čistí sám (nepoužité po 7–30 dnech); ručně jen po gradle --stop, potom se vše stáhne znovu.',
		condition: 'spusť gradle --stop'
	},
	{
		id: 'gradle-config',
		vzory: ['%userprofile%/.gradle/gradle.properties', '%userprofile%/.gradle/init.d'],
		level: 'data',
		certainty: 'medium',
		why: 'Tvoje globální nastavení Gradle (často i přístupové údaje). Nemazat.'
	}
];

/// Zkompilované záznamy pro mandatory.js. Regexy se staví jednou při
/// načtení modulu, ne při každém dotazu — systemPathInfo běží pro
/// každý řádek seznamu při každém překreslení.
export const PATH_KB = ZAZNAMY.map((z) => ({
	...z,
	re: z.vzory.map((v) => vzor(v, z.jenSlozka))
}));
