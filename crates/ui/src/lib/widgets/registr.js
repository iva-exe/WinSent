// Katalog widgetů pro Home.
//
// Jedno místo, kde je o každé dlaždici řečeno všechno: jak se jmenuje,
// z jaké sekce pochází, jaká data potřebuje a jak velká má být.
// Rozložení (rozlozeni.svelte.js) i nabídka Přidat z toho čtou;
// samotné kreslení je v komponentách vedle.
//
// Widget NENÍ zmenšená sekce. Ukazuje jedno číslo nebo krátký seznam
// a v hlavičce má odkaz do sekce, kde je to celé. Co se do dlaždice
// nevejde, tam nepatří.
//
// Pole `sady` jsou klíče z data.svelte.js. Dlaždice se na ně přihlásí
// sama a několik dlaždic nad touž sadou stojí jeden dotaz, ne pět.

import {
	Cpu,
	MemoryStick,
	Zap,
	ChartLine,
	Grid3x3,
	Flame,
	Gauge,
	Clock,
	Clock4,
	HardDrive,
	Thermometer,
	CircuitBoard,
	Microchip,
	Monitor,
	Battery,
	Layers,
	FolderTree,
	Copy,
	FileSearch,
	Blocks,
	Ghost,
	PackagePlus,
	RotateCw,
	Shield,
	Ear,
	Mic,
	TriangleAlert,
	ListStart,
	Power,
	Users,
	History,
	ArrowDownUp,
	Router,
	Network,
	Wifi,
	Signal,
	Globe,
	Database,
	BrainCircuit,
	Minus
} from 'lucide-svelte';

import KartyTasks from './KartyTasks.svelte';
import KartyHardware from './KartyHardware.svelte';
import KartyDisk from './KartyDisk.svelte';
import KartyProgramy from './KartyProgramy.svelte';
import KartyBezpeci from './KartyBezpeci.svelte';
import KartySit from './KartySit.svelte';
import KartyRozvrzeni from './KartyRozvrzeni.svelte';

/// Šířka se počítá ve sloupcích mřížky, výška v řádcích.
///
/// Řádek je schválně nízký: výška se táhne za spodní hranu dlaždice
/// a v krocích po jednom celém widgetu by se nedala doladit. Běžná
/// dlaždice je proto vysoká dva řádky, oddělovač jeden.
export const RADEK = 54;
export const MEZERA = 10;
export const MAX_VYSKA = 12;

function w(id, nazev, sekce, href, ikona, popis, komp, sady, vychozi, extra = {}) {
	return {
		id,
		nazev,
		sekce,
		href,
		ikona,
		popis,
		komp,
		typ: id,
		sady,
		/// [sloupce, řádky]
		vychozi,
		/// Nejmenší rozumná velikost — pod ní se z dlaždice nedá nic vyčíst.
		min: extra.min ?? [1, 2],
		/// Smí být na ploše víckrát? (oddělovače ano, měřáky ne)
		vice: extra.vice ?? false,
		/// Kreslí se bez rámu karty?
		holy: extra.holy ?? false
	};
}

const SEZNAM = [
	// -- Rozvržení ----------------------------------------------------
	w('oddelovac', 'Oddělovač', 'Rozvržení', null, Minus, 'nadpis přes zvolený počet sloupců', KartyRozvrzeni, [], [4, 1], {
		min: [1, 1],
		vice: true,
		holy: true
	}),

	// -- Tasks --------------------------------------------------------
	w('cpu', 'CPU', 'Tasks', '/tasks', Cpu, 'vytížení procesoru a takt', KartyTasks, ['system'], [1, 2]),
	w('ram', 'Paměť', 'Tasks', '/tasks', MemoryStick, 'kolik RAM je obsazené', KartyTasks, ['system'], [1, 2]),
	w('gpu', 'GPU', 'Tasks', '/tasks', Zap, 'vytížení grafiky, VRAM a teplota', KartyTasks, ['system'], [1, 2]),
	w('graf', 'Živý průběh', 'Tasks', '/tasks', ChartLine, 'posledních pár minut systému, CPU, RAM, GPU nebo sítě', KartyTasks, ['system'], [4, 4], { min: [2, 3] }),
	w('jadra', 'Jádra', 'Tasks', '/tasks', Grid3x3, 'zátěž jednotlivých jader', KartyTasks, ['system'], [2, 2]),
	w('zrouti', 'Žrouti', 'Tasks', '/tasks', Flame, 'kdo právě bere nejvíc, s přepínačem metriky', KartyTasks, ['procs', 'system'], [2, 4], { min: [2, 3] }),
	w('seka', 'Proč to seká', 'Tasks', '/tasks', Gauge, 'hard faulty, fronta disku, throttling', KartyTasks, ['system'], [1, 2]),
	w('uptime', 'Běh systému', 'Tasks', '/tasks', Clock, 'jak dlouho běží a kolik toho drží', KartyTasks, ['system'], [1, 2]),
	w('diskrychlost', 'Disky teď', 'Tasks', '/tasks', HardDrive, 'čtení a zápis na fyzických discích', KartyTasks, ['system', 'sysInfo'], [2, 2]),
	w('samotny', 'Winsent sám', 'Tasks', '/tasks', Database, 'co spotřebuje samotný Winsent', KartyTasks, ['samotny'], [1, 2]),

	// -- Hardware -----------------------------------------------------
	w('deska', 'Deska a BIOS', 'Hardware', '/hardware', CircuitBoard, 'model desky a verze firmwaru', KartyHardware, ['hardware'], [2, 2]),
	w('teploty', 'Teploty', 'Hardware', '/hardware', Thermometer, 'co ze součástek teplotu hlásí', KartyHardware, ['system', 'volumes', 'hardware'], [2, 2]),
	w('moduly', 'Paměťové moduly', 'Hardware', '/hardware', Microchip, 'osazené sloty a takt', KartyHardware, ['sysInfo'], [2, 3]),
	w('obrazovky', 'Obrazovky', 'Hardware', '/hardware', Monitor, 'rozlišení a obnovovací frekvence', KartyHardware, ['displays'], [2, 2]),
	w('pagefile', 'Stránkovací soubor', 'Hardware', '/hardware', Layers, 'co se odkládá na disk místo do RAM', KartyHardware, ['hardware'], [1, 2]),
	w('baterie', 'Baterie', 'Hardware', '/hardware', Battery, 'stav a opotřebení, jen na notebooku', KartyHardware, ['hardware'], [1, 2]),
	w('ovladace', 'Ovladače', 'Drivers', '/drivers', BrainCircuit, 'kolik jich je zvenčí a kolik hlásí problém', KartyHardware, ['drivers'], [2, 2]),

	// -- Files / disk -------------------------------------------------
	w('svazky', 'Obsazenost disků', 'Files', '/files', HardDrive, 'kolik místa zbývá na každém svazku', KartyDisk, ['volumes'], [2, 3]),
	w('smart', 'Zdraví disků', 'Files', '/files', Gauge, 'opotřebení a odsloužené hodiny', KartyDisk, ['volumes'], [2, 2]),
	w('velke', 'Co zabírá místo', 'Files', '/files', FolderTree, 'největší složky a soubory', KartyDisk, ['cleanup'], [2, 4], { min: [2, 3] }),
	w('duplicity', 'Duplicity', 'Files', '/files', Copy, 'kolik místa drží kopie téhož', KartyDisk, ['cleanup'], [1, 2]),
	w('indexy', 'Stav indexů', 'Vyhledávání', '/search', FileSearch, 'na kterých discích už hledání funguje', KartyDisk, ['cleanup', 'volumes'], [2, 3]),

	// -- Programs -----------------------------------------------------
	w('naposledy', 'Naposledy otevřené', 'Vyhledávání', '/search', Clock4, 'zkratka na to, co jsi otevíral z hledání', KartyProgramy, [], [2, 4], { min: [2, 2] }),
	w('inventar', 'Inventář', 'Programs', '/programs', Blocks, 'kolik je nainstalováno a kolik běží', KartyProgramy, ['apps', 'system'], [2, 2]),
	w('duchove', 'Duchové po programech', 'Programs', '/programs', Ghost, 'zápisy bez souborů na disku', KartyProgramy, ['apps'], [1, 2]),
	w('nove', 'Nově přibylo', 'Programs', '/programs', PackagePlus, 'co se nainstalovalo naposledy', KartyProgramy, ['apps'], [2, 3]),
	w('sken', 'Inventář aplikací', 'Programs', '/programs', RotateCw, 'kdy proběhl sken a ruční přeskenování', KartyProgramy, [], [1, 2]),

	// -- Security, incidenty, start, účty -----------------------------
	w('ochrana', 'Ochrana v kostce', 'Security', '/security', Shield, 'antivirus, firewall, UAC, šifrování', KartyBezpeci, ['security'], [2, 4], { min: [2, 2] }),
	w('poslouchaji', 'Kdo teď poslouchá', 'Security', '/security', Ear, 'mikrofon, kamera a poloha právě teď', KartyBezpeci, ['security'], [2, 2]),
	// Historie použití zná jen cestu k programu; čitelné jméno je až
	// v seznamu oprávnění, proto se odebírají obě sady.
	w('mikrofon', 'Mikrofon za týden', 'Security', '/security', Mic, 'kdo ho držel nejdéle', KartyBezpeci, ['permUse', 'security'], [2, 3]),
	w('incident', 'Poslední incident', 'Incidents', '/incidents', TriangleAlert, 'co se stalo naposledy a kdy', KartyBezpeci, ['incidents'], [2, 2]),
	w('incidenty30', 'Incidenty za 30 dní', 'Incidents', '/incidents', TriangleAlert, 'kolik záseků a pádů bylo za měsíc', KartyBezpeci, ['incidents'], [1, 2]),
	w('startup', 'Startuje s Windows', 'On start', '/onstart', ListStart, 'přepínání položek po startu', KartyBezpeci, ['startup'], [2, 4], { min: [2, 3] }),
	w('startpocet', 'Kolik toho startuje', 'On start', '/onstart', Power, 'souhrn položek po startu', KartyBezpeci, ['startup'], [1, 2]),
	w('ucty', 'Účty', 'Users', '/users', Users, 'kdo je tu správce', KartyBezpeci, ['users'], [2, 2]),
	w('audit', 'Co Winsent udělal', 'Historie', '/history', History, 'provedené a zamítnuté akce', KartyBezpeci, ['audit'], [2, 3]),

	// -- Síť ----------------------------------------------------------
	w('prenos', 'Přenos teď', 'Network', '/network', ArrowDownUp, 'download a upload v reálném čase', KartySit, ['system'], [1, 2]),
	w('stahuje', 'Kdo teď stahuje', 'Network', '/network', Router, 'aplikace podle stahování', KartySit, ['network'], [2, 4], { min: [2, 2] }),
	w('spojeni', 'Aktivní spojení', 'Network', '/network', Network, 'kolik jich je a kolik aplikací je drží', KartySit, ['network'], [1, 2]),
	w('porty', 'Naslouchající porty', 'Network', '/network', Ear, 'otevřené brány mimo tento počítač', KartySit, ['network'], [2, 3]),
	w('linka', 'Linka', 'Connection', '/connection', Wifi, 'kterou kartou jsi připojený', KartySit, ['connection'], [2, 2]),
	w('adresa', 'Adresa v síti', 'Connection', '/connection', Globe, 'IP, brána a DNS', KartySit, ['connection'], [2, 2]),
	w('signal', 'Signál WiFi', 'Connection', '/connection', Signal, 'síla připojené sítě', KartySit, ['connection'], [1, 2]),
	w('site', 'Sítě v dosahu', 'Connection', '/connection', Wifi, 'co je kolem vidět', KartySit, ['connection'], [2, 3])
];

/// Widgety podle id.
export const REGISTR = Object.fromEntries(SEZNAM.map((it) => [it.id, it]));

/// Co je na ploše, dokud si uživatel nic nevybral.
///
/// Rovnou i s oddělovači — jednak je to nejrychlejší způsob, jak
/// ukázat, že tam patří, jednak je pak plocha čitelná i bez úprav.
export function vychoziRozlozeni() {
	return [
		{ id: 'cpu', w: 1, h: 2 },
		{ id: 'ram', w: 1, h: 2 },
		{ id: 'gpu', w: 1, h: 2 },
		{ id: 'prenos', w: 1, h: 2 },
		{ id: 'graf', w: 4, h: 4 },
		{ id: 'zrouti', w: 2, h: 4 },
		{ id: 'svazky', w: 2, h: 4 },
		{ id: 'oddelovac', w: 4, h: 1, text: 'Bezpečnost' },
		{ id: 'ochrana', w: 2, h: 4 },
		{ id: 'incident', w: 2, h: 2 },
		{ id: 'startpocet', w: 1, h: 2 },
		{ id: 'ucty', w: 1, h: 2 },
		{ id: 'oddelovac', w: 4, h: 1, text: 'Síť a programy' },
		{ id: 'stahuje', w: 2, h: 4 },
		{ id: 'inventar', w: 2, h: 2 },
		{ id: 'naposledy', w: 2, h: 4 },
		{ id: 'linka', w: 2, h: 2 }
	];
}
