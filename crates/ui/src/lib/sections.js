// Sekce aplikace na jednom místě.
//
// Čte odsud navigace i jejich zapínání v Nastavení. Dva seznamy by se
// rozešly hned, jak přibude sekce — a v Nastavení by chyběla právě ta
// nová, tedy ta, u které je volba nejužitečnější.
//
// Nastavení samo v seznamu SCHVÁLNĚ není: kdyby šlo vypnout, nedala by
// se volba vzít zpátky jinak než smazáním úložiště prohlížeče.
import {
	House,
	Activity,
	TriangleAlert,
	Blocks,
	Files,
	Search,
	ListStart,
	Users,
	Cpu,
	BrainCircuit,
	Wifi,
	Router,
	Shield,
	History
} from 'lucide-svelte';

/// Pořadí i názvy jsou závazné (Frame 5, DESIGN.md kap. 6).
/// `dole` = patří k Nastavení do spodní části navigace, ne do seznamu.
export const SEKCE = [
	{ href: '/home', label: 'Home', icon: House },
	{ href: '/tasks', label: 'Tasks', icon: Activity },
	{ href: '/incidents', label: 'Incidents', icon: TriangleAlert },
	{ href: '/programs', label: 'Programs', icon: Blocks },
	{ href: '/files', label: 'Files', icon: Files },
	{ href: '/search', label: 'Vyhledávání', icon: Search },
	{ href: '/onstart', label: 'On start', icon: ListStart },
	{ href: '/users', label: 'Users', icon: Users },
	{ href: '/hardware', label: 'Hardware', icon: Cpu },
	{ href: '/drivers', label: 'Drivers', icon: BrainCircuit },
	{ href: '/connection', label: 'Connection', icon: Wifi },
	{ href: '/network', label: 'Network', icon: Router },
	{ href: '/security', label: 'Security', icon: Shield },
	{ href: '/history', label: 'Historie', icon: History, dole: true }
];
