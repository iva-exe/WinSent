// Brána: záznam o počítači nesmí prozradit jméno uživatele.
//
//   node crates/ui/tests/maskcheck.mjs
//
// Maska cest se dřív zastavila na první mezeře, takže u účtu „Jan Novak"
// (profil C:\Users\Jan Novak) zůstalo v souboru příjmení jako
// `C:\Users\<uživatel> Novak`. Soubor se přitom posílá e-mailem a do
// ticketů. Případy níž jsou přesně ty, na kterých to prasklo.
//
// Testuje se přes reportText(), ne přes vnitřní funkci — zajímá nás
// výsledný soubor, ne implementace.

import { reportText } from '../src/lib/pcreport.js';

const now = 1787788000;

const UCTY = {
	current_user: 'Jan Novak',
	admin_group: 'Administrators',
	users: [
		{ name: 'Jan Novak', full_name: 'Jan Novak', admin: true },
		{ name: 'IVA', full_name: '' }
	]
};

// [cesta, popis, očekávaný výsledek]
const PRIPADY = [
	[
		'C:\\Users\\Jan Novak\\AppData\\Roaming\\Spotify\\Spotify.exe --autostart',
		'jméno s mezerou',
		'C:\\Users\\<uživatel>\\AppData\\Roaming\\Spotify\\Spotify.exe --autostart'
	],
	[
		'"C:\\Users\\Jan Novak\\AppData\\Local\\Discord\\Update.exe"',
		'jméno s mezerou v uvozovkách',
		'"C:\\Users\\<uživatel>\\AppData\\Local\\Discord\\Update.exe"'
	],
	[
		'C:\\Users\\IVA\\AppData\\Roaming\\Movavi\\x.exe',
		'jméno bez mezery',
		'C:\\Users\\<uživatel>\\AppData\\Roaming\\Movavi\\x.exe'
	],
	[
		'C:/Users/IVA/AppData/Roaming\\Movavi\\x.exe',
		'lomítka dopředu (tak to zapsal instalátor)',
		'C:/Users/<uživatel>/AppData/Roaming\\Movavi\\x.exe'
	],
	[
		'\\\\server\\Users\\Jan Novak\\sdilene\\x.exe',
		'síťová cesta bez písmene disku',
		'\\\\server\\Users\\<uživatel>\\sdilene\\x.exe'
	],
	[
		'C:\\Users\\Neznamy Ucet\\x.exe',
		'profil, který neodpovídá žádnému účtu',
		'C:\\Users\\<uživatel>\\x.exe'
	],
	// Složky pod C:\Users, které nepatří člověku — maskovat je nemá co
	// chránit a jen to znečistí výstup.
	[
		'C:\\Users\\All Users\\Microsoft\\x.exe',
		'sdílená složka All Users',
		'C:\\Users\\All Users\\Microsoft\\x.exe'
	],
	[
		'C:\\Users\\Public\\Desktop\\x.lnk',
		'sdílená složka Public',
		'C:\\Users\\Public\\Desktop\\x.lnk'
	],
	// Cesta bez pokračování, za kterou je věta: maskuje se jen jméno,
	// zbytek hlášky musí zůstat čitelný.
	[
		'C:\\Users\\IVA je spatne nastavena a proto to nejde',
		'cesta a za ní věta',
		'C:\\Users\\<uživatel> je spatne nastavena a proto to nejde'
	]
];

function radekPoSpusteni(cesta, ucty) {
	const txt = reportText({
		now,
		from: now - 86400,
		users: ucty,
		startup: [
			{
				source: 'run_user',
				name: 'Polozka',
				command: cesta,
				enabled: true,
				running: null,
				system: false
			}
		]
	});
	// Legenda v hlavičce obsahuje `C:\Users\Jméno` jako příklad, proto
	// se čte až sekce PO SPUŠTĚNÍ.
	const radky = txt.split('\n');
	const zac = radky.findIndex((l) => l.includes('PO SPUŠTĚNÍ'));
	return (radky.slice(zac).find((l) => /Users/i.test(l)) ?? '').trim();
}

let chyb = 0;
for (const [cesta, popis, ocekavano] of PRIPADY) {
	const je = radekPoSpusteni(cesta, UCTY);
	const ok = je === ocekavano;
	if (!ok) chyb++;
	console.log(`  ${ok ? 'ok  ' : 'CHYBA'}  ${popis}`);
	if (!ok) {
		console.log(`          čekáno: ${ocekavano}`);
		console.log(`          je:     ${je}`);
	}
}

// Bez seznamu účtů (dotaz na účty selhal) se maskovat nesmí přestat.
const bez = radekPoSpusteni('C:\\Users\\IVA\\AppData\\x.exe', null);
if (!bez.includes('<uživatel>')) {
	console.log('  CHYBA  bez seznamu účtů se přestalo maskovat: ' + bez);
	chyb++;
} else {
	console.log('  ok    bez seznamu účtů se maskuje dál');
}

// Detail incidentu a událostí jde do záznamu jako SUROVÝ JSON, kde je
// každé zpětné lomítko zapsané dvakrát. Maska, která trvala na jednom
// oddělovači, takový řádek pustila celý — i se jménem uživatele.
// Naměřeno na detailu pádu Discordu a na události proc_crash.
const JSON_PRIPADY = [
	[
		'detail incidentu',
		{
			incidents: [
				{
					ts: now - 10,
					kind: 'app_crash',
					culprit: 'Discord',
					detail: JSON.stringify({
						exit_code: 3221225477,
						name: 'Discord.exe',
						path: 'C:\\Users\\Jan Novak\\AppData\\Local\\Discord\\Discord.exe'
					})
				}
			]
		}
	],
	[
		'detail události',
		{
			events: [
				{ ts: now - 20, kind: 'proc_crash', pid: 1, detail: 'C:\\\\Users\\\\Jan Novak\\\\x.exe' }
			]
		}
	],
	[
		'cíl v auditu',
		{
			audit: [
				{
					ts: now - 30,
					action: 'delete',
					target: 'C:\\Users\\Jan Novak\\Desktop\\x.txt',
					class: 'T1',
					verdict: 'allow',
					outcome: 'ok'
				}
			]
		}
	],
	[
		'velká písmena v cestě',
		{
			audit: [
				{
					ts: now - 40,
					action: 'delete',
					target: 'C:\\USERS\\JAN NOVAK\\x.txt',
					class: 'T1',
					verdict: 'allow',
					outcome: 'ok'
				}
			]
		}
	]
];

for (const [popis, data] of JSON_PRIPADY) {
	const txt = reportText({ now, from: now - 86400, users: UCTY, ...data });
	// Sekce ÚČTY jméno uvádí ZÁMĚRNĚ (viz legenda v hlavičce záznamu),
	// takže se z kontroly vyjme; jde o cesty.
	const bezUctu = txt.replace(/^.*Přihlášený účet:.*$/gm, '').replace(/^ {2}Jan Novak.*$/gm, '');
	if (/novak/i.test(bezUctu)) {
		console.log('  CHYBA  ' + popis + ': jméno uživatele uniklo');
		bezUctu
			.split('\n')
			.filter((l) => /novak/i.test(l))
			.forEach((l) => console.log('          ' + l.trim()));
		chyb++;
	} else {
		console.log('  ok    ' + popis);
	}
}

// Jméno účtu nesmí zůstat nikde v celém souboru jako součást cesty.
const cely = reportText({
	now,
	from: now - 86400,
	users: UCTY,
	startup: [
		{
			source: 'run_user',
			name: 'Polozka',
			command: 'C:\\Users\\Jan Novak\\AppData\\x.exe',
			enabled: true,
			running: null,
			system: false
		}
	]
});
if (/Users[\\/]Jan/.test(cely)) {
	console.log('  CHYBA  jméno účtu zůstalo v cestě někde v souboru');
	chyb++;
}

// Volný text (hlášení o pádu): maska nesmí komolit časy a verze a musí
// zakrýt adresy celé. Dřívější vzor IPv6 bral i čas 18:04:12, IPv4
// verzi 2.1.0.12, MAC prohnal podruhé (zmizel výrobce) a u fe80::…
// nechal v textu půlku identifikátoru rozhraní.
const VOLNY_TEXT = [
	['spadlo v 18:04:12, verze 1.0.9.164', 'čas a verze zůstanou', 'spadlo v 18:04:12, verze 1.0.9.164'],
	['Verze modulu: 2.1.0.12', 'verze modulu z hlášení o pádu', 'Verze modulu: 2.1.0.12'],
	['{"app_version":"1.2.3.4"}', 'verze v JSONu', '{"app_version":"1.2.3.4"}'],
	['spojení na 192.168.1.20 selhalo', 'IPv4 v textu', 'spojení na 192.168.x.x selhalo'],
	['MAC 9C-6B-00-12-34-56', 'MAC si nechá výrobce', 'MAC 9C:6B:00:xx:xx:xx'],
	['adresa fe80::1c2d:3e4f:5a6b:7c8d%12', 'zkrácená IPv6 celá', 'adresa fe80::…%12'],
	['z 2001:db8:85a3::8a2e:370:7334 port 443', 'IPv6 s prefixem', 'z 2001:db8:… port 443'],
	['2001:0db8:85a3:0000:0000:8a2e:0370:7334', 'plná IPv6', '2001:0db8:…'],
	['naslouchá na :: a ::1', 'nespecifikovaná a loopback zůstanou', 'naslouchá na :: a ::1'],
	// Dvojtečka hned za adresou (chybová hláška) nebo před ní (předpona
	// „IPv6:") dřív maskování zrušila a adresa prošla celá.
	['connect to 2001:db8:85a3::8a2e:370:7334: timed out', 'IPv6 před dvojtečkou', 'connect to 2001:db8:…: timed out'],
	['IPv6:fe80::1c2d:3e4f:5a6b:7c8d', 'IPv6 za předponou', 'IPv6:fe80::…'],
	// „version" uvnitř slova není slovo verze — IP za ním se maskuje.
	['conversion 192.168.1.20 failed', 'conversion není verze', 'conversion 192.168.x.x failed'],
	['konverze 10.1.2.3', 'konverze není verze', 'konverze 10.1.x.x'],
	['člen S-1-12-1-1234567890-123456789-987654321-1122334455', 'SID z Entra', 'člen S-1-12-1-<tenant>'],
	['účet S-1-5-21-111-222-333-1001', 'lokální SID si nechá RID', 'účet S-1-5-21-<stroj>-1001']
];
for (const [vstup, popis, ocekavano] of VOLNY_TEXT) {
	const txt = reportText({
		now,
		from: now - 86400,
		users: UCTY,
		crashes: [{ ts: now - 5, app: 'Pad.exe', repeats: 1, summary: vstup }]
	});
	const radky = txt.split('\n');
	const zac = radky.findIndex((l) => l.includes('Pad.exe'));
	const je = (radky[zac + 1] ?? '').trim();
	const ok = je === ocekavano;
	if (!ok) chyb++;
	console.log(`  ${ok ? 'ok  ' : 'CHYBA'}  volný text: ${popis}`);
	if (!ok) {
		console.log(`          čekáno: ${ocekavano}`);
		console.log(`          je:     ${je}`);
	}
}

console.log(chyb === 0 ? '\nBRÁNA maskcheck: PASS' : `\nBRÁNA maskcheck: FAIL (${chyb})`);
process.exit(chyb === 0 ? 0 : 1);
