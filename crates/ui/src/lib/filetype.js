// Základní druh souboru podle přípony — ikona a slovo do seznamu.
//
// Přípon jsou tisíce, ale ve výsledcích hledání jde jen o to, aby šlo
// od oka poznat obrázek od videa a archiv od programu. Proto pár
// velkých skupin a nic víc: co se netrefí, dostane obecnou ikonu
// souboru. Hádat druh podle jména je horší než nehádat.
//
// Vrací se jen `id` (řetězec), ne komponenta ikony — díky tomu je
// modul čistý JavaScript a jde ho použít i mimo Svelte (třeba
// v kontextovém menu nebo v testu).

/// `id` skupiny → přípony, které do ní patří.
const SKUPINY = {
	obrazek: [
		'png',
		'jpg',
		'jpeg',
		'gif',
		'bmp',
		'webp',
		'svg',
		'ico',
		'tif',
		'tiff',
		'heic',
		'avif',
		'raw',
		'cr2',
		'nef',
		'psd'
	],
	video: ['mp4', 'mkv', 'avi', 'mov', 'wmv', 'webm', 'flv', 'm4v', 'mpg', 'mpeg', 'ts', '3gp'],
	zvuk: ['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'wma', 'opus', 'mid', 'midi'],
	archiv: ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'zst', 'iso', 'cab', 'jar'],
	dokument: ['pdf', 'doc', 'docx', 'odt', 'rtf', 'txt', 'md', 'epub', 'mobi', 'log'],
	tabulka: ['xls', 'xlsx', 'csv', 'ods', 'tsv'],
	prezentace: ['ppt', 'pptx', 'odp'],
	kod: [
		'js',
		'ts',
		'jsx',
		'tsx',
		'rs',
		'py',
		'c',
		'cpp',
		'cc',
		'h',
		'hpp',
		'cs',
		'java',
		'kt',
		'go',
		'rb',
		'php',
		'swift',
		'html',
		'css',
		'scss',
		'json',
		'xml',
		'yml',
		'yaml',
		'toml',
		'ini',
		'sql',
		'svelte',
		'vue'
	],
	skript: ['bat', 'cmd', 'ps1', 'sh', 'vbs', 'reg'],
	program: ['exe', 'msi', 'msix', 'appx', 'lnk', 'com'],
	knihovna: ['dll', 'sys', 'drv', 'ocx', 'so'],
	databaze: ['db', 'sqlite', 'sqlite3', 'mdb', 'accdb', 'dat']
};

/// Slovní popis skupiny — jde do tooltipu, ať ikona nemusí nic „znamenat".
const POPIS = {
	obrazek: 'obrázek',
	video: 'video',
	zvuk: 'zvuk',
	archiv: 'archiv',
	dokument: 'dokument',
	tabulka: 'tabulka',
	prezentace: 'prezentace',
	kod: 'zdrojový kód',
	skript: 'skript',
	program: 'program',
	knihovna: 'systémová knihovna',
	databaze: 'databáze',
	soubor: 'soubor'
};

/// Přípona → skupina. Postaví se jednou; hledání v seznamech pro
/// každý řádek výsledků by bylo zbytečně drahé.
const PODLE_PRIPONY = (() => {
	const m = new Map();
	for (const [id, pripony] of Object.entries(SKUPINY)) {
		for (const p of pripony) m.set(p, id);
	}
	return m;
})();

/// Přípona jména souboru bez tečky (malými písmeny), nebo prázdno.
export function pripona(name = '') {
	const i = name.lastIndexOf('.');
	if (i <= 0 || i === name.length - 1) return '';
	return name.slice(i + 1).toLowerCase();
}

/// Druh souboru: `{ id, popis, pripona }`. Pro složku se nevolá.
export function typSouboru(name = '') {
	const p = pripona(name);
	const id = PODLE_PRIPONY.get(p) ?? 'soubor';
	return { id, popis: POPIS[id], pripona: p };
}
