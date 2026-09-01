// Katalog widgetů pro Home.
//
// Jedno místo, kde je o každé dlaždici řečeno všechno: jak se jmenuje,
// z jaké sekce pochází, jaká data potřebuje a v jakých velikostech dává
// smysl. Rozložení (rozlozeni.svelte.js) i nabídka Přidat z toho čtou;
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
	BrainCircuit
} from 'lucide-svelte';

import KartyTasks from './KartyTasks.svelte';
import KartyHardware from './KartyHardware.svelte';
import KartyDisk from './KartyDisk.svelte';
import KartyProgramy from './KartyProgramy.svelte';
import KartyBezpeci from './KartyBezpeci.svelte';
import KartySit from './KartySit.svelte';

/// Velikosti v jednotkách mřížky. Pořadí je zároveň pořadím, ve kterém
/// se přepínají tlačítkem na dlaždici.
export const VELIKOSTI = {
	mala: { w: 1, h: 1, popis: 'malá' },
	stredni: { w: 2, h: 1, popis: 'široká' },
	vysoka: { w: 1, h: 2, popis: 'vysoká' },
	velka: { w: 2, h: 2, popis: 'velká' },
	siroka: { w: 4, h: 2, popis: 'přes celou' }
};

function w(id, nazev, sekce, href, ikona, popis, komp, sady, velikosti, vychozi) {
	return { id, nazev, sekce, href, ikona, popis, komp, typ: id, sady, velikosti, vychozi };
}

const SEZNAM = [
	// -- Tasks --------------------------------------------------------
	w('cpu', 'CPU', 'Tasks', '/tasks', Cpu, 'vytížení procesoru a takt', KartyTasks, ['system'], ['mala', 'stredni'], 'mala'),
	w('ram', 'Paměť', 'Tasks', '/tasks', MemoryStick, 'kolik RAM je obsazené', KartyTasks, ['system'], ['mala', 'stredni'], 'mala'),
	w('gpu', 'GPU', 'Tasks', '/tasks', Zap, 'vytížení grafiky, VRAM a teplota', KartyTasks, ['system'], ['mala', 'stredni'], 'mala'),
	w('graf', 'Živý průběh', 'Tasks', '/tasks', ChartLine, 'posledních pár minut CPU, RAM, GPU nebo sítě', KartyTasks, ['system'], ['stredni', 'velka', 'siroka'], 'siroka'),
	w('jadra', 'Jádra', 'Tasks', '/tasks', Grid3x3, 'zátěž jednotlivých jader', KartyTasks, ['system'], ['stredni', 'velka'], 'stredni'),
	w('zrouti', 'Žrouti', 'Tasks', '/tasks', Flame, 'kdo právě bere nejvíc, s přepínačem metriky', KartyTasks, ['procs'], ['stredni', 'velka'], 'velka'),
	w('seka', 'Proč to seká', 'Tasks', '/tasks', Gauge, 'hard faulty, fronta disku, throttling', KartyTasks, ['system'], ['mala', 'stredni'], 'mala'),
	w('uptime', 'Běh systému', 'Tasks', '/tasks', Clock, 'jak dlouho běží a kolik toho drží', KartyTasks, ['system'], ['mala', 'stredni'], 'mala'),
	w('diskrychlost', 'Disky teď', 'Tasks', '/tasks', HardDrive, 'čtení a zápis na fyzických discích', KartyTasks, ['system', 'sysInfo'], ['mala', 'stredni'], 'stredni'),
	w('samotny', 'Winsent sám', 'Tasks', '/tasks', Database, 'co spotřebuje samotný Winsent', KartyTasks, ['samotny'], ['mala', 'stredni'], 'mala'),

	// -- Hardware -----------------------------------------------------
	w('deska', 'Deska a BIOS', 'Hardware', '/hardware', CircuitBoard, 'model desky a verze firmwaru', KartyHardware, ['hardware'], ['stredni', 'velka'], 'stredni'),
	w('teploty', 'Teploty', 'Hardware', '/hardware', Thermometer, 'co ze součástek teplotu hlásí', KartyHardware, ['system', 'volumes', 'hardware'], ['mala', 'stredni'], 'stredni'),
	w('moduly', 'Paměťové moduly', 'Hardware', '/hardware', Microchip, 'osazené sloty a takt', KartyHardware, ['sysInfo'], ['stredni', 'velka'], 'stredni'),
	w('obrazovky', 'Obrazovky', 'Hardware', '/hardware', Monitor, 'rozlišení a obnovovací frekvence', KartyHardware, ['displays'], ['mala', 'stredni'], 'stredni'),
	w('pagefile', 'Stránkovací soubor', 'Hardware', '/hardware', Layers, 'co se odkládá na disk místo do RAM', KartyHardware, ['hardware'], ['mala', 'stredni'], 'mala'),
	w('baterie', 'Baterie', 'Hardware', '/hardware', Battery, 'stav a opotřebení, jen na notebooku', KartyHardware, ['hardware'], ['mala', 'stredni'], 'mala'),
	w('ovladace', 'Ovladače', 'Drivers', '/drivers', BrainCircuit, 'kolik jich je zvenčí a kolik hlásí problém', KartyHardware, ['drivers'], ['mala', 'stredni'], 'stredni'),

	// -- Files / disk -------------------------------------------------
	w('svazky', 'Obsazenost disků', 'Files', '/files', HardDrive, 'kolik místa zbývá na každém svazku', KartyDisk, ['volumes'], ['stredni', 'velka'], 'stredni'),
	w('smart', 'Zdraví disků', 'Files', '/files', Gauge, 'opotřebení a odsloužené hodiny', KartyDisk, ['volumes'], ['mala', 'stredni'], 'stredni'),
	w('velke', 'Co zabírá místo', 'Files', '/files', FolderTree, 'největší složky a soubory', KartyDisk, ['cleanup'], ['velka', 'siroka'], 'velka'),
	w('duplicity', 'Duplicity', 'Files', '/files', Copy, 'kolik místa drží kopie téhož', KartyDisk, ['cleanup'], ['mala', 'stredni'], 'mala'),
	w('indexy', 'Stav indexů', 'Vyhledávání', '/search', FileSearch, 'na kterých discích už hledání funguje', KartyDisk, ['cleanup', 'volumes'], ['stredni', 'velka'], 'stredni'),

	// -- Programs -----------------------------------------------------
	w('naposledy', 'Naposledy otevřené', 'Vyhledávání', '/search', Clock4, 'zkratka na to, co jsi otevíral z hledání', KartyProgramy, [], ['stredni', 'velka'], 'velka'),
	w('inventar', 'Inventář', 'Programs', '/programs', Blocks, 'kolik je nainstalováno a kolik běží', KartyProgramy, ['apps', 'system'], ['mala', 'stredni'], 'stredni'),
	w('duchove', 'Duchové po programech', 'Programs', '/programs', Ghost, 'zápisy bez souborů na disku', KartyProgramy, ['apps'], ['mala', 'stredni'], 'mala'),
	w('nove', 'Nově přibylo', 'Programs', '/programs', PackagePlus, 'co se nainstalovalo naposledy', KartyProgramy, ['apps'], ['stredni', 'velka'], 'stredni'),
	w('sken', 'Inventář aplikací', 'Programs', '/programs', RotateCw, 'kdy proběhl sken a ruční přeskenování', KartyProgramy, [], ['mala', 'stredni'], 'mala'),

	// -- Security, incidenty, start, účty -----------------------------
	w('ochrana', 'Ochrana v kostce', 'Security', '/security', Shield, 'antivirus, firewall, UAC, šifrování', KartyBezpeci, ['security'], ['stredni', 'velka'], 'velka'),
	w('poslouchaji', 'Kdo teď poslouchá', 'Security', '/security', Ear, 'mikrofon, kamera a poloha právě teď', KartyBezpeci, ['security'], ['mala', 'stredni'], 'stredni'),
	// Historie použití zná jen cestu k programu; čitelné jméno je až
	// v seznamu oprávnění, proto se odebírají obě sady.
	w('mikrofon', 'Mikrofon za týden', 'Security', '/security', Mic, 'kdo ho držel nejdéle', KartyBezpeci, ['permUse', 'security'], ['stredni', 'velka'], 'stredni'),
	w('incident', 'Poslední incident', 'Incidents', '/incidents', TriangleAlert, 'co se stalo naposledy a kdy', KartyBezpeci, ['incidents'], ['stredni', 'velka'], 'stredni'),
	w('incidenty30', 'Incidenty za 30 dní', 'Incidents', '/incidents', TriangleAlert, 'kolik záseků a pádů bylo za měsíc', KartyBezpeci, ['incidents'], ['mala', 'stredni'], 'mala'),
	w('startup', 'Startuje s Windows', 'On start', '/onstart', ListStart, 'přepínání položek po startu', KartyBezpeci, ['startup'], ['velka', 'siroka'], 'velka'),
	w('startpocet', 'Kolik toho startuje', 'On start', '/onstart', Power, 'souhrn položek po startu', KartyBezpeci, ['startup'], ['mala', 'stredni'], 'mala'),
	w('ucty', 'Účty', 'Users', '/users', Users, 'kdo je tu správce', KartyBezpeci, ['users'], ['mala', 'stredni'], 'stredni'),
	w('audit', 'Co Winsent udělal', 'Historie', '/history', History, 'provedené a zamítnuté akce', KartyBezpeci, ['audit'], ['stredni', 'velka'], 'stredni'),

	// -- Síť ----------------------------------------------------------
	w('prenos', 'Přenos teď', 'Network', '/network', ArrowDownUp, 'download a upload v reálném čase', KartySit, ['system'], ['mala', 'stredni'], 'mala'),
	w('stahuje', 'Kdo teď stahuje', 'Network', '/network', Router, 'aplikace podle stahování', KartySit, ['network'], ['stredni', 'velka'], 'velka'),
	w('spojeni', 'Aktivní spojení', 'Network', '/network', Network, 'kolik jich je a kolik aplikací je drží', KartySit, ['network'], ['mala', 'stredni'], 'mala'),
	w('porty', 'Naslouchající porty', 'Network', '/network', Ear, 'otevřené brány mimo tento počítač', KartySit, ['network'], ['stredni', 'velka'], 'stredni'),
	w('linka', 'Linka', 'Connection', '/connection', Wifi, 'kterou kartou jsi připojený', KartySit, ['connection'], ['stredni', 'velka'], 'stredni'),
	w('adresa', 'Adresa v síti', 'Connection', '/connection', Globe, 'IP, brána a DNS', KartySit, ['connection'], ['mala', 'stredni'], 'stredni'),
	w('signal', 'Signál WiFi', 'Connection', '/connection', Signal, 'síla připojené sítě', KartySit, ['connection'], ['mala', 'stredni'], 'mala'),
	w('site', 'Sítě v dosahu', 'Connection', '/connection', Wifi, 'co je kolem vidět', KartySit, ['connection'], ['stredni', 'velka'], 'stredni')
];

/// Widgety podle id.
export const REGISTR = Object.fromEntries(SEZNAM.map((it) => [it.id, it]));

/// Co je na ploše, dokud si uživatel nic nevybral.
///
/// Zhruba to, co Home ukazoval dřív, plus průběh -- ať po aktualizaci
/// nikdo nekouká na prázdnou plochu a zároveň hned vidí, že se dlaždice
/// dají skládat.
export function vychoziRozlozeni() {
	return [
		{ id: 'cpu', velikost: 'mala' },
		{ id: 'ram', velikost: 'mala' },
		{ id: 'gpu', velikost: 'mala' },
		{ id: 'prenos', velikost: 'mala' },
		{ id: 'graf', velikost: 'siroka' },
		{ id: 'zrouti', velikost: 'velka' },
		{ id: 'svazky', velikost: 'stredni' },
		{ id: 'incident', velikost: 'stredni' },
		{ id: 'ochrana', velikost: 'velka' },
		{ id: 'stahuje', velikost: 'velka' },
		{ id: 'inventar', velikost: 'stredni' },
		{ id: 'naposledy', velikost: 'stredni' }
	];
}
