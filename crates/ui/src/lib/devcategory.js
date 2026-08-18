// Kategorie zařízení — hrubé dělení podle toho, co zařízení znamená pro
// uživatele, ne podle tříd Windows. Nikoho nezajímá, že klávesnice je
// „HIDClass".
//
// Sdílené mezi Hardwarem a Ovladači SCHVÁLNĚ. Dokud to byla tabulka
// v jednom souboru, znamenalo přidání třídy dvě úpravy na dvou místech —
// a druhé se zapomene. Ovladač i zařízení navíc nesou tatáž pole
// (`class`, `class_desc`), takže sdílet jde i ta funkce, ne jen data.

import {
	AudioLines,
	Boxes,
	Cog,
	Cpu,
	Keyboard,
	Monitor,
	Network,
	Printer,
	Usb
} from 'lucide-svelte';

/// Pořadí je pořadím na obrazovce: od toho, co uživatele zajímá nejvíc.
export const CATEGORIES = [
	{ name: 'Komponenty', icon: Cpu },
	{ name: 'Obrazovky', icon: Monitor },
	{ name: 'Periferie', icon: Keyboard },
	{ name: 'Zvuk', icon: AudioLines },
	{ name: 'Síť', icon: Network },
	{ name: 'Řadiče a porty', icon: Usb },
	{ name: 'Tisk', icon: Printer },
	{ name: 'Systémová zařízení', icon: Cog },
	{ name: 'Ostatní', icon: Boxes }
];

export const CLASS_CATEGORY = {
	Processor: 'Komponenty',
	Display: 'Komponenty',
	DiskDrive: 'Komponenty',
	Monitor: 'Obrazovky',
	Keyboard: 'Periferie',
	Mouse: 'Periferie',
	HIDClass: 'Periferie',
	WPD: 'Periferie',
	Image: 'Periferie',
	Camera: 'Periferie',
	Bluetooth: 'Periferie',
	Biometric: 'Periferie',
	MEDIA: 'Zvuk',
	AudioEndpoint: 'Zvuk',
	AudioProcessingObject: 'Zvuk',
	Net: 'Síť',
	USB: 'Řadiče a porty',
	HDC: 'Řadiče a porty',
	SCSIAdapter: 'Řadiče a porty',
	Ports: 'Řadiče a porty',
	Volume: 'Řadiče a porty',
	FloppyDisk: 'Řadiče a porty',
	PrintQueue: 'Tisk',
	Printer: 'Tisk',
	PrinterPort: 'Tisk',
	System: 'Systémová zařízení',
	Computer: 'Systémová zařízení',
	Firmware: 'Systémová zařízení',
	SoftwareDevice: 'Systémová zařízení',
	Battery: 'Komponenty',
	BasicDisplay: 'Komponenty',
	VolumeSnapshot: 'Řadiče a porty',
	ScmVolume: 'Řadiče a porty',
	SmrVolume: 'Řadiče a porty',
	CDROM: 'Řadiče a porty',
	USBDevice: 'Řadiče a porty',
	Sensor: 'Periferie',
	SmartCardReader: 'Periferie',
	// U balíků DCH (NVIDIA, Intel, AMD) jsou tyhle dvě třídy početné —
	// jsou to doplňky k hlavnímu ovladači, ne samostatná zařízení.
	SoftwareComponent: 'Systémová zařízení',
	Extension: 'Systémová zařízení',
	SecurityDevices: 'Systémová zařízení'
};

/// Do které kategorie položka patří.
///
/// `item` potřebuje `class` a `class_desc`. `usbLike` říká, že jde
/// o zařízení pořízené přes USB (má VID/PID) — u výrobcem vymyšlených
/// tříd jako „Razer Device" je to jediné vodítko, že je to periferie.
///
/// Proč zvlášť parametrem: Hardware to pozná z `hardware_id`, Ovladače
/// z prefixu `dev:` ve `group_key`. Kdyby se to četlo natvrdo z
/// `hardware_id`, byla by tahle větev u ovladačů mrtvá a totéž
/// zařízení by v Hardwaru bylo „Periferie" a v Driverech „Ostatní".
export function categoryOf(item, usbLike = false) {
	const known = CLASS_CATEGORY[item.class];
	if (known) return known;
	// Výrobci si zakládají vlastní třídy („Focusrite Audio",
	// „Razer Device"), takže seznam tříd nestačí.
	const cls = ((item.class ?? '') + ' ' + (item.class_desc ?? '')).toLowerCase();
	if (cls.includes('audio') || cls.includes('zvuk')) return 'Zvuk';
	if (cls.includes('net') || cls.includes('síť')) return 'Síť';
	if (cls.includes('print') || cls.includes('tisk')) return 'Tisk';
	if (usbLike) return 'Periferie';
	return 'Ostatní';
}

/// Má položka VID/PID? Hardware to nese v `hardware_id`.
export function hasVidPid(id) {
	const s = (id ?? '').toUpperCase();
	return s.includes('VID_') && s.includes('PID_');
}

/// Rozdělí položky do kategorií a vrátí jen ty neprázdné, v pořadí
/// podle CATEGORIES. Klíč sekce nese i pořadí — dvakrát stejný klíč
/// ve {#each} je v produkčním buildu Svelte tvrdá chyba, která zabije
/// překreslování celé stránky.
export function byCategory(items, usbLikeOf = () => false) {
	const map = new Map(CATEGORIES.map((c) => [c.name, []]));
	for (const it of items) {
		map.get(categoryOf(it, usbLikeOf(it)))?.push(it);
	}
	return CATEGORIES.filter((c) => map.get(c.name).length).map((c, i) => ({
		key: `${i}:${c.name}`,
		name: c.name,
		icon: c.icon,
		items: map.get(c.name)
	}));
}
