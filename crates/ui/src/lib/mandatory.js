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
