// Brána: záznam o počítači nesmí tvrdit nic, co z dat neplyne.
//
//   node crates/ui/tests/reportcheck.mjs
//
// Každý případ níž je chyba, která se v záznamu opravdu objevila:
//   · „Aktualizace: služba ručně“ na stroji, kde se instaluje samo
//     (wuauserv má od Windows 10 Start = 3 i při plné automatice),
//   · „TPM: nenalezen“ tam, kde čip je a jen WMI mlčí,
//     a stejně tak mlčení místo „BitLocker: nezjištěno“,
//   · 118 řádků pro jeden Discord, protože ConsentStore klíčuje
//     oprávnění cestou a každá verze si založila vlastní záznam,
//   · chybějící závěr o konci podpory, přestože sestavení v záznamu je.

import { reportText } from '../src/lib/pcreport.js';

const now = Math.floor(Date.parse('2026-08-27T00:00:00Z') / 1000);

function opravneni() {
	const out = [];
	// Jedna aplikace, 118 záznamů v registru — reálně naměřeno.
	for (let i = 0; i < 118; i++) {
		out.push({
			capability: 'microphone',
			app: `C:\\Users\\IVA\\AppData\\Local\\Discord\\app-1.0.${9000 + i}\\Discord.exe`,
			app_name: 'Discord',
			group_key: 'c:\\users\\iva\\appdata\\local\\discord\\*\\discord.exe',
			allow: true,
			enforced: false,
			in_use: i === 117,
			last_used: now - i * 3600
		});
	}
	out.push({
		capability: 'webcam',
		app: 'C:\\Program Files\\obs-studio\\bin\\64bit\\obs64.exe',
		app_name: 'obs64',
		group_key: 'c:\\program files\\obs-studio\\bin\\64bit\\obs64.exe',
		allow: false,
		enforced: false,
		in_use: false,
		last_used: null
	});
	return out;
}

function zaznam(zmeny = {}) {
	const perms = opravneni();
	const os = {
		product: 'Windows 10 Pro',
		display_version: '22H2',
		build: 19045,
		ubr: 7663,
		arch: 'x64',
		install_ts: 1603185792,
		update_last_search: now - 3600,
		update_last_install: null,
		update_service_start: 3,
		update_disabled_by_policy: false,
		...(zmeny.os ?? {})
	};
	return reportText({
		now,
		from: now - 86400,
		users: { current_user: 'IVA', users: [{ name: 'IVA' }] },
		security: {
			protection: {
				av: [],
				defender: null,
				fw_domain: true,
				fw_private: true,
				fw_public: true,
				uac_enabled: true,
				uac_admin_prompt: 2,
				secure_boot: null,
				tpm: zmeny.tpm === undefined ? [false, ''] : zmeny.tpm,
				encryption: zmeny.encryption ?? [],
				os
			},
			permissions: perms
		},
		permTotals: perms.map((p) => [p.app, p.capability, 600])
	});
}

const zakladni = zaznam();

/// Záznam s hardwarovou sekcí — kvůli tepelné kaskádě.
function zaznamHw(thermal) {
	return reportText({
		now,
		from: now - 86400,
		users: { current_user: 'IVA', users: [{ name: 'IVA' }] },
		hw: {
			cpu_thermal: { clock_mhz: 3800, max_mhz: 4200, throttling: false, ...thermal },
			disks: [],
			volumes: [],
			devices: []
		}
	});
}

const KONTROLY = [
	// Verze téže aplikace patří na jeden řádek.
	['oprávnění: verze sloučené do jednoho řádku', () => /Discord {2}\(118 verzí\)/.test(zakladni)],
	[
		'oprávnění: počet záznamů v registru je přiznaný',
		() => /118 záznamů v registru/.test(zakladni)
	],
	[
		'oprávnění: sekce se vešla pod 40 řádků',
		() => {
			const r = zakladni.split('\n');
			const z = r.findIndex((l) => l.includes('Oprávnění aplikací'));
			const k = r.slice(z).findIndex((l) => l.includes('nevynucují'));
			return k > 0 && k < 40;
		}
	],
	// Typ spuštění wuauserv není režim aktualizací.
	[
		'aktualizace: Start = 3 není falešný poplach',
		() => /Aktualizace: zapnuté/.test(zakladni) && !/služba ručně/.test(zakladni)
	],
	[
		'aktualizace: zakázaná služba se pozná',
		() => /Aktualizace: VYPNUTÉ/.test(zaznam({ os: { update_service_start: 4 } }))
	],
	[
		'aktualizace: zákaz zásadou se pozná',
		() => /VYPNUTÉ.*NoAutoUpdate/.test(zaznam({ os: { update_disabled_by_policy: true } }))
	],
	// Nevím se nesmí vydávat za ne.
	['TPM: prázdná specifikace znamená nezjištěno', () => /čip je, stav nezjištěn/.test(zakladni)],
	[
		'TPM: chybějící čip se pozná od nezjištěného',
		() => /nenalezen \(žádný čip/.test(zaznam({ tpm: null }))
	],
	['BitLocker: prázdný seznam neznamená mlčení', () => /BitLocker: {2}nezjištěno/.test(zakladni)],
	[
		'BitLocker: vyplněný svazek se vypíše',
		() => /BitLocker: {2}C: — zapnuté/.test(zaznam({ encryption: [['C:', 1]] }))
	],
	// Závěr, pro který data v záznamu byla už dřív.
	[
		'konec podpory: Windows 10 22H2 se ohlásí',
		() => /po konci podpory \(2025-10-14\)/.test(zakladni)
	],
	[
		'konec podpory: podporovaná verze mlčí',
		() =>
			!/konci podpory/.test(
				zaznam({
					os: { build: 26100, display_version: '24H2', product: 'Windows 11 Enterprise' }
				})
			)
	],
	// Termín závisí na edici, ne jen na sestavení. Enterprise 23H2 je
	// dnes podporované, Home/Pro téhož sestavení už ne.
	[
		'konec podpory: Enterprise má vlastní termín',
		() => {
			const t = zaznam({
				os: { build: 22631, display_version: '23H2', product: 'Windows 11 Enterprise' }
			});
			return /má konec podpory 2026-11-10/.test(t) && !/je po konci podpory/.test(t);
		}
	],
	[
		'konec podpory: Home/Pro téhož sestavení je po termínu',
		() =>
			/je po konci podpory \(2025-11-11\)/.test(
				zaznam({ os: { build: 22631, display_version: '23H2', product: 'Windows 11 Pro' } })
			)
	],
	// LTSC a IoT mají termíny podle konkrétního vydání — hádat je by
	// znamenalo falešný poplach na strojích podporovaných do roku 2032.
	[
		'konec podpory: LTSC mlčí',
		() =>
			!/konci podpory/.test(
				zaznam({
					os: { build: 19044, display_version: '21H2', product: 'Windows 10 IoT Enterprise LTSC' }
				})
			)
	],
	[
		'konec podpory: u Windows 10 se přizná výjimka ESU',
		() => /ESU/.test(zakladni)
	],
	// Windows teplotu jádra nevydávají a Winsent do jádra nesahá.
	// Řádek „teplota — °C (zdroj nedostupné)" nebyl údaj, jen šum.
	[
		'teplota: nedostupná se nevypisuje jako pomlčka',
		() => {
			const t = zaznamHw({ celsius: null, temp_source: 'nedostupné' });
			return !/teplota — °C/.test(t) && /Windows samy nevydávají/.test(t);
		}
	],
	[
		'teplota: naměřená se vypíše i se zdrojem',
		() =>
			/teplota 54 °C \(zdroj HWiNFO\)/.test(
				zaznamHw({ celsius: 53.6, temp_source: 'HWiNFO' })
			)
	]
];

let chyb = 0;
for (const [popis, test] of KONTROLY) {
	let ok = false;
	try {
		ok = test();
	} catch (e) {
		ok = false;
		console.log('          výjimka: ' + e.message);
	}
	if (!ok) chyb++;
	console.log(`  ${ok ? 'ok  ' : 'CHYBA'}  ${popis}`);
}

console.log(chyb === 0 ? '\nBRÁNA reportcheck: PASS' : `\nBRÁNA reportcheck: FAIL (${chyb})`);
process.exit(chyb === 0 ? 0 : 1);
