<script>
	// Vyhledávání — jádro sekce Vyhledávání i spotlight lišty.
	//
	// Jedna komponenta pro obojí schválně: kdyby si lišta kreslila
	// vlastní seznam, rozešly by se v chování dvě věci, které jsou pro
	// uživatele táž funkce.
	//
	// Hledá se ve dvou zdrojích naráz:
	//   • nainstalované aplikace (inventář služby) — ty jdou první,
	//     protože „napsat kus jména a zmáčknout Enter" je nejčastější
	//     důvod, proč si člověk takovou lištu vyvolá,
	//   • soubory a složky v MFT indexu (SPEC 11.2). Index čte tabulku
	//     souborů NTFS napřímo, takže výsledky chodí v desítkách
	//     milisekund i pro miliony souborů — to je to, co dělá rychlým
	//     „Everything" a co běžné hledání ve Windows nedokáže.
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import {
		Search,
		Folder,
		File,
		Loader,
		CornerDownLeft,
		Clock,
		Image,
		Film,
		Music,
		FileArchive,
		FileText,
		FileSpreadsheet,
		Presentation,
		FileCode,
		Terminal,
		AppWindow,
		Binary,
		Database,
		PackageX,
		Play
	} from 'lucide-svelte';
	import AppIcon from '$lib/AppIcon.svelte';
	import SystemBadge from '$lib/SystemBadge.svelte';
	import { isSystemApp, systemPathInfo } from '$lib/mandatory.js';
	import { typSouboru } from '$lib/filetype.js';
	import { nacti as nactiPosledni, zapamatuj, zapomen, zapomenJednu } from '$lib/recent.js';
	import { openMenu, akceKopirovat, oddelovac } from '$lib/itemmenu.svelte.js';

	let {
		/// Kompaktní podoba pro spotlight lištu.
		compact = false,
		/// Zavolá se, když uživatel akci dokončil (lišta se pak schová).
		onhotovo = () => {}
	} = $props();

	let query = $state('');
	let hits = $state([]);
	let apps = $state([]);
	let spustitelne = $state([]);
	let procs = $state([]);
	let posledni = $state([]);
	let busy = $state(false);
	let chyba = $state('');
	let vybrany = $state(0);
	let vstup = $state(null);
	let chipyEl = $state(null);
	let svazky = $state([]);
	let indexStav = $state([]);
	let filtr = $state('vse');

	/// FILE_ATTRIBUTE_DIRECTORY.
	const ATTR_DIR = 0x10;
	/// Kolik výsledků se tahá ze služby. Víc než se vejde na obrazovku
	/// nemá smysl — kdo hledá, upřesní dotaz, nescrolluje tisíce řádků.
	const LIMIT = 200;
	/// Kolik aplikací se vejde nad soubory ve zobrazení „Vše". Aplikace
	/// jdou první, ale nesmí vytlačit soubory z první obrazovky —
	/// koho zajímají jen ony, přepne se filtrem a uvidí všechny.
	const APPS_VE_VSEM = 6;

	// ── Ikony ────────────────────────────────────────────────────────
	const IKONY_TYPU = {
		obrazek: Image,
		video: Film,
		zvuk: Music,
		archiv: FileArchive,
		dokument: FileText,
		tabulka: FileSpreadsheet,
		prezentace: Presentation,
		kod: FileCode,
		skript: Terminal,
		program: AppWindow,
		knihovna: Binary,
		databaze: Database,
		soubor: File
	};
	// Barvy jsou tlumené a je jich málo: v seznamu o padesáti řádcích
	// dělá duha šum, ne informaci. Jde jen o to, aby šel obrázek od
	// videa poznat periferním viděním.
	const BARVY_TYPU = {
		obrazek: 'var(--net-up)',
		video: 'var(--net-up)',
		zvuk: 'var(--net-down)',
		archiv: 'var(--warn)',
		tabulka: 'var(--ok)',
		prezentace: 'var(--warn)',
		kod: 'var(--net-down)',
		skript: 'var(--net-down)',
		program: 'var(--ok)',
		databaze: 'var(--net-down)'
	};

	// ── Data od služby ───────────────────────────────────────────────
	async function nacistSvazky() {
		try {
			const c = await invoke('query_cleanup');
			indexStav = c?.indexing ?? [];
			// Svazek, jehož index selhal, má taky hotovo=true — jen
			// k tomu důvod. Prohledat se nedá, takže do seznamu ani
			// mezi přepínače nepatří.
			svazky = indexStav.filter(([, , hotovo, chyba]) => hotovo && !chyba).map(([l]) => l);
			zahrej();
		} catch {
			svazky = [];
		}
	}

	// Ohřátí indexu při otevření vyhledávání.
	//
	// Služba index po nečinnosti uvolňuje z paměti a hledání by ho pak
	// muselo obstarat až uvnitř dotazu. Otevření lišty je na to nejlepší
	// chvíle: než člověk napíše druhý znak, je index připravený a
	// odpověď chodí v jednotkách milisekund. Odpověď nás nezajímá —
	// jde jen o to, ať se služba probere dřív než uživatel dopíše.
	let ohratoMs = 0;
	function zahrej() {
		if (Date.now() - ohratoMs < 30_000) return;
		ohratoMs = Date.now();
		for (const l of svazky) {
			invoke('build_file_index', { letter: l }).catch(() => {});
		}
	}

	// Inventář, spustitelné položky a procesy se drží v paměti a
	// obnovují jen občas: seznam aplikací se mění po instalaci, ne mezi
	// dvěma stisky klávesy.
	let nactenoAppsMs = 0;
	async function nacistApps(force = false) {
		if (!force && Date.now() - nactenoAppsMs < 60_000) return;
		try {
			apps = await invoke('query_apps');
			// Razítko až po úspěchu. Když je služba při startu ještě
			// dole, nesmí se minutu čekat s prázdným seznamem programů.
			nactenoAppsMs = Date.now();
		} catch {
			/* služba mimo — zkusí se při příštím otevření */
		}
		try {
			spustitelne = await invoke('query_launchables');
		} catch {
			/* starší hostitel ten příkaz nezná — zbude inventář */
		}
		try {
			procs = await invoke('query_procs');
		} catch {
			procs = [];
		}
	}

	// Ikony aplikací — stejný mechanismus jako v Programs a Tasks
	// (RGBA → canvas → data URL), ale tahají se jen pro řádky, které
	// jsou zrovna vidět. Načíst ikonu ke každé nainstalované aplikaci
	// by při každém stisku klávesy znamenalo stovky dotazů přes pipe.
	let iconUrls = $state({});
	const iconState = new Map();

	function rgbaToUrl(icon) {
		const c = document.createElement('canvas');
		c.width = icon.w;
		c.height = icon.h;
		const ctx = c.getContext('2d');
		ctx.putImageData(new ImageData(new Uint8ClampedArray(icon.rgba), icon.w, icon.h), 0, 0);
		return c.toDataURL();
	}

	async function fetchIcon(it) {
		if (iconState.has(it.key)) return;
		iconState.set(it.key, 'bezi');
		try {
			let icon = null;
			if (it.identity_key) {
				icon = await invoke('query_icon', { identityKey: it.identity_key });
			}
			// Služba ikonu nemá vždycky: její cache je po startu skoro
			// minutu prázdná a u programů bez instalační složky nemá
			// odkud brát (naměřeno 357 ikon ze 478 aplikací). Shell si
			// ikonu vyrobí sám a hned — z balíčku, z .exe i ze zástupce.
			if (!icon && it.aumid) {
				icon = await invoke('query_launchable_icon', { aumid: it.aumid });
			}
			if (icon) iconUrls[it.key] = rgbaToUrl(icon);
		} catch {
			// Nezapamatovat si neúspěch — příště to může vyjít.
			iconState.delete(it.key);
		}
	}

	// ── Hledání ──────────────────────────────────────────────────────
	let dotaz = $derived(query.trim());

	// Hledá se se zpožděním: psaní je rychlejší než dotaz přes pipe
	// a bez tlumení by se posílal dotaz na každé písmeno. Aplikace se
	// filtrují v paměti, takže naskočí okamžitě — lišta tím působí
	// svižně i ve chvíli, kdy soubory ještě letí po pipe.
	let timer;
	function napsano() {
		clearTimeout(timer);
		// Zneplatnit i dotaz, který už letí. Bez toho se po smazání
		// znaku na ~90 ms vrátí výsledky předchozího, delšího dotazu —
		// a to je přesně to, co má čítač běhů hlídat.
		beh++;
		vybrany = 0;
		if (dotaz.length < 2) {
			hits = [];
			busy = false;
			return;
		}
		busy = true;
		timer = setTimeout(hledat, 90);
	}

	// Číslo běhu — odpověď staršího dotazu nesmí přepsat novější
	// výsledky. Uživatel píše rychle a odpovědi chodí na přeskáčku.
	let beh = 0;

	async function hledat() {
		const q = dotaz;
		const muj = ++beh;
		try {
			// Napříč všemi svazky naráz; služba je má v paměti zvlášť.
			// Chyba jednoho svazku nesmí shodit ostatní, ale ani zmizet:
			// dřív se tiše měnila na prázdné pole a hledání pak jen
			// tvrdilo „nic se nenašlo".
			const davky = await Promise.all(
				svazky.map((l) =>
					invoke('search_files', { letter: l, query: q, limit: LIMIT })
						.then((r) => ({ l, r }))
						.catch((e) => ({ l, r: [], e: String(e) }))
				)
			);
			if (muj !== beh) return;
			const spatne = davky.filter((d) => d.e);
			chyba = spatne.length
				? `${spatne.map((d) => d.l + ':').join(' ')} se nepodařilo prohledat — ${spatne[0].e}`
				: '';
			// Složky napřed, pak podle délky cesty: co je blíž kořeni,
			// bývá to hledané. Uvnitř abecedně, ať pořadí neposkakuje.
			// Bez seskupování: složky a soubory jdou v jedné řadě.
			// Rozhoduje, jak blízko je nález hledanému, ne co to je za
			// typ — složka „Zálohy" nemá být nad souborem „Zaloha.txt"
			// jen proto, že je to složka.
			const qn = bezDiakritiky(q);
			hits = davky
				.flatMap((d) => d.r)
				.map((h) => ({ h, k: poradiSouboru(h, qn) }))
				.sort((a, b) => {
					for (let i = 0; i < a.k.length; i++) {
						if (a.k[i] !== b.k[i]) return a.k[i] - b.k[i];
					}
					return a.h.name.localeCompare(b.h.name, 'cs');
				})
				.map((x) => x.h)
				.slice(0, LIMIT);
		} catch (e) {
			if (muj !== beh) return;
			chyba = String(e);
			hits = [];
		}
		if (muj === beh) busy = false;
	}

	/// Kolik procesů běží pod kterou aplikací.
	let bezici = $derived.by(() => {
		const m = new Map();
		for (const p of procs) m.set(p.identity_key, (m.get(p.identity_key) ?? 0) + 1);
		return m;
	});

	/// Kolik procesů běží pod kterým jménem obrazu (`chrome.exe`).
	/// Pro soubor na disku je to jediné spojení se živým během —
	/// ProcRow nese jméno bez cesty, ne celou cestu.
	let beziciObrazy = $derived.by(() => {
		const m = new Map();
		for (const p of procs) {
			const n = (p.name ?? '').toLowerCase();
			if (n) m.set(n, (m.get(n) ?? 0) + 1);
		}
		return m;
	});

	/// Text bez diakritiky, malými písmeny.
	///
	/// Na české klávesnici se hledá „kalkulacka", ne „Kalkulačka" —
	/// hlavně v liště, kde jde o rychlost. Rozklad na NFD a zahození
	/// spojovacích znamének zachovává DÉLKU řetězce (jeden znak
	/// s háčkem → jeden bez), takže se podle indexů z porovnání dá
	/// bez přepočtu krájet původní text pro zvýraznění.
	function bezDiakritiky(t) {
		return (t ?? '')
			.toLowerCase()
			.normalize('NFD')
			.replace(/\p{M}/gu, '');
	}

	/// Skóre shody jména aplikace s dotazem: 0 = začíná jím,
	/// 1 = začíná jím některé slovo, 2 = obsahuje ho, −1 = neshoda.
	function skore(jmeno, q) {
		const i = jmeno.indexOf(q);
		if (i < 0) return -1;
		if (i === 0) return 0;
		return /[\s\-_.(]/.test(jmeno[i - 1]) ? 1 : 2;
	}

	/// Skóre celé položky. Kromě jména se prohledá i vydavatel a
	/// strojové jméno balíčku, ale s výrazně horším skóre: kdo napíše
	/// „spotify", má dostat Spotify, ne všechno od Spotify AB.
	function skorePolozky(it, q) {
		const j = skore(bezDiakritiky(it.name), q);
		if (j >= 0) return j;
		const v = skore(bezDiakritiky(it.sub), q);
		if (v >= 0) return v + 3;
		const k = skore(bezDiakritiky(it.identity_key), q);
		return k >= 0 ? k + 3 : -1;
	}

	/// Klíč řazení nálezu v souborech.
	///
	/// Nejdřív úplná shoda jména, pak POLOHA hledaného v názvu: co jím
	/// začíná, patří výš. Na dotaz „al" tak vyjde „Aluminium" nad
	/// „Zákal". Při stejné poloze vyhrává kratší jméno (míň přílepků
	/// kolem hledaného) a nakonec cesta blíž ke kořeni.
	///
	/// Služba už výsledky takhle seřadila, ale každý svazek odpovídá
	/// zvlášť — tohle je slévá do jednoho pořadí.
	function poradiSouboru(h, qn) {
		const jmeno = bezDiakritiky(h.name);
		const kde = jmeno.indexOf(qn);
		return [
			jmeno === qn ? 0 : 1,
			// Index svazku porovnává diakritiku jinak než my, takže se
			// shoda nemusí najít; takový nález patří dozadu, ne pryč.
			kde < 0 ? 9999 : kde,
			h.name.length,
			h.path.split(/[\\/]/).length
		];
	}

	/// Jméno jako klíč pro párování dvou zdrojů: malá písmena, slova
	/// oddělená mezerou. Stejné pravidlo jako v hostiteli (launch.rs).
	function klicJmena(s) {
		return bezDiakritiky(s)
			.replace(/[^\p{L}\p{N}]+/gu, ' ')
			.trim();
	}

	/// Programy pro hledání: inventář služby + složka „Aplikace".
	///
	/// Ani jeden zdroj sám nestačí. Inventář ví, co je nainstalované,
	/// kolik to zabírá a kolik procesů z toho běží. Složka „Aplikace"
	/// (tatáž, ze které bere nabídka Start) ví, co se dá spustit a jak
	/// se to jmenuje česky. Naměřeno na jednom stroji: ze 260
	/// spustitelných položek jich inventář neznal 178 — aplikace ze
	/// Storu pod strojovým jménem („Microsoft.WindowsCalculator" místo
	/// „Kalkulačka"), programy nainstalované jen pro přihlášeného
	/// uživatele, vestavěné nástroje Windows, hry i přenosné programy.
	let programy = $derived.by(() => {
		const podleKlice = new Map();
		const podleJmena = new Map();
		const podlePrvniho = new Map();
		for (const a of apps) {
			if (a.identity_key) podleKlice.set(a.identity_key, a);
			const n = klicJmena(a.display_name);
			if (!n) continue;
			if (!podleJmena.has(n)) podleJmena.set(n, a);
			const prvni = n.slice(0, n.indexOf(' ') < 0 ? n.length : n.indexOf(' '));
			if (!podlePrvniho.has(prvni)) podlePrvniho.set(prvni, []);
			podlePrvniho.get(prvni).push(a);
		}

		const pouzite = new Set();
		const out = [];
		for (const sp of spustitelne) {
			// Zástupce na soubor, který na disku není, nemá co nabízet.
			if (sp.missing) continue;
			const n = klicJmena(sp.name);
			let inv = sp.identity_key ? podleKlice.get(sp.identity_key) : null;
			if (!inv) inv = podleJmena.get(n) ?? null;
			// Inventář nese verzi a architekturu („Blockbench 4.12.4"),
			// nabídka Start holé jméno. Bez druhého průchodu by se
			// každý takový program ukázal dvakrát.
			if (!inv && n) {
				const kandidati = (podlePrvniho.get(n.slice(0, n.indexOf(' ') < 0 ? n.length : n.indexOf(' '))) ?? [])
					.filter((a) => klicJmena(a.display_name).startsWith(n + ' '))
					.sort((a, b) => a.display_name.length - b.display_name.length);
				inv = kandidati[0] ?? null;
			}
			if (inv && pouzite.has(inv.identity_key)) inv = null;
			if (inv) pouzite.add(inv.identity_key);
			out.push(polozkaApp(inv, sp));
		}
		// Co v nabídce Start není, ale nainstalované to je: runtimy,
		// SDK, ovladače. Spustit se nedají, ale hledat ano — a nesou
		// štítky o chybějící instalaci.
		for (const a of apps) {
			if (!pouzite.has(a.identity_key)) out.push(polozkaApp(a, null));
		}
		return out;
	});

	/// Aplikace odpovídající dotazu, seřazené podle shody. Filtruje se
	/// v paměti — je jich pár set a dotaz přes pipe by byl pomalejší
	/// než celé porovnání.
	let appHits = $derived.by(() => {
		if (dotaz.length < 2) return [];
		const q = bezDiakritiky(dotaz);
		return programy
			.map((it) => ({ it, s: skorePolozky(it, q) }))
			.filter((x) => x.s >= 0)
			.sort(
				(x, y) =>
					x.s - y.s ||
					x.it.name.length - y.it.name.length ||
					x.it.name.localeCompare(y.it.name, 'cs')
			)
			.map((x) => x.it);
	});

	// ── Jednotná položka seznamu ─────────────────────────────────────
	// Aplikace, soubory, složky i „naposledy otevřené" se kreslí týmž
	// řádkem. Jinak by se tři skoro stejné šablony rozešly v detailech.
	/// `a` je řádek inventáře, `sp` položka nabídky Start; aspoň jedno
	/// z nich musí být. Jméno vyhrává to ze Startu — je lokalizované
	/// a bez verze, tedy to, co uživatel opravdu napíše.
	function polozkaApp(a, sp) {
		return {
			kind: 'app',
			key: a ? `app:${a.identity_key}` : `launch:${sp.aumid}`,
			name: sp?.name ?? a.display_name,
			sub: a?.publisher ?? '',
			path: '',
			identity_key: a?.identity_key ?? '',
			aumid: sp?.aumid ?? '',
			attrs: 0,
			disk: '',
			size: null,
			missing: !!a?.missing_install,
			system: a ? isSystemApp(a) : !!sp?.system,
			systemInfo: null
		};
	}

	function polozkaSoubor(h) {
		const dir = (h.attrs & ATTR_DIR) !== 0;
		return {
			kind: dir ? 'dir' : 'file',
			key: `f:${h.path}`,
			aumid: '',
			name: h.name,
			sub: slozka(h.path),
			path: h.path,
			identity_key: '',
			attrs: h.attrs,
			disk: h.path[1] === ':' ? h.path[0].toUpperCase() : '',
			size: h.size_bytes ?? null,
			missing: false,
			system: false,
			systemInfo: systemPathInfo(h.path)
		};
	}

	/// Uložený záznam zpátky na položku. Doplní se, co se od minula
	/// mohlo změnit (systémová značka, počet procesů) — zbytek je,
	/// jak si ho uživatel otevřel.
	function polozkaZPosledni(r) {
		return {
			kind: r.kind,
			key: r.key,
			name: r.name,
			sub: r.sub,
			path: r.path,
			identity_key: r.identity_key,
			aumid: r.aumid,
			attrs: r.attrs,
			disk: r.disk,
			size: null,
			missing: false,
			system:
				r.kind === 'app' &&
				isSystemApp({
					identity_key: r.identity_key,
					display_name: r.name,
					publisher: r.sub
				}),
			systemInfo: r.path ? systemPathInfo(r.path) : null,
			nedavne: true
		};
	}

	/// Kolik instancí položky běží.
	function pocetBehu(it) {
		if (it.kind === 'app') return bezici.get(it.identity_key) ?? 0;
		if (it.kind === 'file') return beziciObrazy.get(it.name.toLowerCase()) ?? 0;
		return 0;
	}

	// ── Filtry ───────────────────────────────────────────────────────
	// Vše / Programy / Soubory / Složky / Disk X. Přepíná se TABem,
	// takže ruka nemusí z klávesnice — u lišty, která se vyvolává
	// zkratkou, by myš byla krok zpátky.
	let filtry = $derived([
		{ id: 'vse', label: 'Vše' },
		{ id: 'app', label: 'Programy' },
		{ id: 'file', label: 'Soubory' },
		{ id: 'dir', label: 'Složky' },
		...svazky.map((l) => ({ id: `disk:${l}`, label: `Disk ${l}` }))
	]);

	function projde(it, f) {
		if (f === 'vse') return true;
		if (f.startsWith('disk:')) return it.disk === f.slice(5);
		return it.kind === f;
	}

	/// Všechno, co dotaz našel — bez omezení, jen pro počty u filtrů.
	/// Prázdný řádek ukáže naposledy otevřené; jeden znak ještě nic,
	/// jinak by při každém prvním písmenu problikl seznam z historie.
	let vse = $derived.by(() => {
		if (!dotaz) return posledni.map(polozkaZPosledni);
		if (dotaz.length < 2) return [];
		return [...appHits, ...hits.map(polozkaSoubor)];
	});

	/// Co by seznam ukázal pro daný filtr. Ve „Vše" se aplikace
	/// ořezávají, ať nevytlačí soubory z první obrazovky.
	///
	/// Počty u přepínačů jdou přes tutéž funkci schválně: dokud se
	/// počítaly zvlášť, hlásil přepínač „Vše 240" nad seznamem
	/// o 206 řádcích a dvě čísla na jedné obrazovce si odporovala.
	function proFiltr(zdroj, f) {
		const v = zdroj.filter((it) => projde(it, f));
		// Bez dotazu je v seznamu historie a ta má vlastní pořadí: od
		// naposledy otevřeného. Přerovnat ji tak, aby byly programy
		// nahoře, by z „co jsem měl posledně" udělalo „co je čím".
		if (f !== 'vse' || !dotaz) return v;
		const a = v.filter((it) => it.kind === 'app').slice(0, APPS_VE_VSEM);
		return [...a, ...v.filter((it) => it.kind !== 'app')];
	}

	let vysledky = $derived(proFiltr(vse, filtr));

	let pocty = $derived.by(() => {
		const m = new Map();
		for (const f of filtry) m.set(f.id, proFiltr(vse, f.id).length);
		return m;
	});

	/// Výběr se ořezává, ne přepisuje: seznam se pod rukama mění
	/// (dobíhá hledání, přepne se filtr) a zápis do stavu z efektu by
	/// se s tím honil.
	let vyber = $derived(Math.min(vybrany, Math.max(0, vysledky.length - 1)));

	function nastavFiltr(id) {
		filtr = id;
		vybrany = 0;
		vstup?.focus();
	}

	function posunFiltr(krok) {
		const i = filtry.findIndex((f) => f.id === filtr);
		const n = filtry.length;
		nastavFiltr(filtry[((i < 0 ? 0 : i) + krok + n) % n].id);
	}

	// Aktivní filtr musí zůstat vidět, i když se pruh nevejde na řádek.
	$effect(() => {
		filtr;
		queueMicrotask(() => {
			chipyEl
				?.querySelector('.fs-chip.on')
				?.scrollIntoView({ inline: 'nearest', block: 'nearest', behavior: 'smooth' });
		});
	});

	// Filtr, který zmizel (odpojený disk), by seznam vyprázdnil beze
	// stopy — proto se v takovém případě vrací „Vše".
	$effect(() => {
		if (!filtry.some((f) => f.id === filtr)) filtr = 'vse';
	});

	// Ikony jen pro to, co je zrovna na obrazovce.
	$effect(() => {
		for (const it of vysledky) {
			if (it.kind === 'app') fetchIcon(it);
		}
	});

	// ── Pomocné ──────────────────────────────────────────────────────
	function fmtSize(b) {
		if (b == null) return '';
		if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB';
		if (b >= 1e6) return (b / 1e6).toFixed(1) + ' MB';
		if (b >= 1e3) return (b / 1e3).toFixed(0) + ' kB';
		return b + ' B';
	}

	/// Cesta bez posledního dílu — ten je už v názvu.
	function slozka(p) {
		const i = Math.max(p.lastIndexOf('\\'), p.lastIndexOf('/'));
		if (i < 0) return p;
		// Kořen svazku si musí nechat lomítko: „C:" místo „C:\\" vypadá
		// jako uříznutá cesta.
		if (i === 2 && p[1] === ':') return p.slice(0, 3);
		return p.slice(0, i) || p;
	}

	/// Zvýrazní část názvu, která odpovídá dotazu. Bez toho není poznat,
	/// proč se řádek objevil.
	function casti(text) {
		if (!dotaz) return [{ t: text, m: false }];
		// Porovnává se bez diakritiky, ale krájí se PŮVODNÍ text —
		// jinak by se v „Kalkulačka" zvýraznilo „Kalkulacka".
		const i = bezDiakritiky(text).indexOf(bezDiakritiky(dotaz));
		if (i < 0) return [{ t: text, m: false }];
		return [
			{ t: text.slice(0, i), m: false },
			{ t: text.slice(i, i + dotaz.length), m: true },
			{ t: text.slice(i + dotaz.length), m: false }
		].filter((c) => c.t);
	}

	// ── Akce ─────────────────────────────────────────────────────────
	/// Klíč položky, která se zrovna spouští.
	///
	/// Najít aplikaci ve složce „Aplikace" trvá až půl sekundy (naměřeno
	/// 260 položek ≈ 530 ms). Bez viditelného stavu vypadá lišta po
	/// Enteru zamrzle a uživatel mačká znovu.
	let spousti = $state('');

	async function otevrit(it) {
		if (spousti) return;
		spousti = it.key;
		try {
			if (it.kind === 'app') {
				await invoke('launch_app', {
					identityKey: it.identity_key,
					displayName: it.name,
					// Když položku známe z nabídky Start, spustí se
					// přesně ona — žádné dohledávání podle jména.
					aumid: it.aumid || null
				});
			} else {
				await invoke('open_path', { path: it.path });
			}
			posledni = zapamatuj(it);
			chyba = '';
		} catch (e) {
			// Lišta se schválně nezavírá: uživatel by hlášku nestihl
			// přečíst a vypadalo by to, že se prostě nic nestalo.
			chyba = String(e);
			return;
		} finally {
			spousti = '';
		}
		onhotovo();
	}

	function klavesa(e) {
		if (e.key === 'Tab') {
			e.preventDefault();
			posunFiltr(e.shiftKey ? -1 : 1);
		} else if (e.key === 'ArrowDown') {
			e.preventDefault();
			// Math.max(0, …) je tu kvůli prázdnému seznamu: bez něj
			// šipka dolů nastavila −1 a odvozený výběr se z toho už
			// nedostal — žádný řádek nebyl vybraný a Enter nedělal nic.
			vybrany = Math.max(0, Math.min(vyber + 1, vysledky.length - 1));
			doHledu();
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			vybrany = Math.max(vyber - 1, 0);
			doHledu();
		} else if (e.key === 'Enter') {
			e.preventDefault();
			if (vysledky[vyber]) otevrit(vysledky[vyber]);
		}
	}

	// Vybraný řádek musí zůstat vidět při ovládání z klávesnice.
	//
	// Zároveň se tím odstaví myš: po odrolování pošle prohlížeč
	// mouseenter řádku, který se ocitl pod nehybným kurzorem, a ten by
	// výběr přepsal — při držení šipky dolů se výběr „lepil" k myši
	// místo aby šel dolů. Myš se vrátí ke slovu, jakmile se pohne.
	let klavesniceVede = false;
	function doHledu() {
		klavesniceVede = true;
		queueMicrotask(() => {
			document.querySelector('.fs-row.sel')?.scrollIntoView({ block: 'nearest' });
		});
	}

	/// Položky menu, které se týkají historie. Ukazují se jen u řádků,
	/// které z historie opravdu jsou — u výsledku hledání by nedávaly
	/// smysl. Odebrat jednu je zvlášť od vymazání všeho: kvůli jednomu
	/// omylem otevřenému souboru nemá uživatel přijít o celý seznam.
	function akceHistorie(it) {
		if (!it.nedavne) return [];
		return [
			oddelovac,
			{
				label: 'Odebrat z historie',
				icon: 'trash',
				hint: it.name,
				run: () => (posledni = zapomenJednu(it.key))
			},
			{
				label: 'Vymazat celou historii',
				icon: 'trash',
				danger: true,
				run: () => (posledni = zapomen())
			}
		];
	}

	function menuPolozky(e, it) {
		if (it.kind === 'app') {
			openMenu(e, {
				title: it.name,
				subtitle: it.sub,
				hledat: [it.name, it.sub],
				kontext: 'program pro Windows',
				items: [
					{ label: 'Spustit', icon: 'open', run: () => otevrit(it) },
					oddelovac,
					akceKopirovat(it.name, 'Kopírovat název'),
					...akceHistorie(it)
				]
			});
			return;
		}
		const t = it.kind === 'dir' ? null : typSouboru(it.name);
		openMenu(e, {
			title: it.name,
			subtitle: it.sub,
			// Celá cesta se do vyhledávače neposílá — je v ní jméno
			// uživatele a struktura jeho disku.
			hledat: [it.name],
			kontext: it.kind === 'dir' ? 'složka Windows' : 'soubor',
			items: [
				{ label: 'Otevřít v Průzkumníku', icon: 'folder', run: () => otevrit(it) },
				t?.pripona
					? {
							label: `Co je přípona .${t.pripona}?`,
							icon: 'search',
							hint: `.${t.pripona}`,
							run: () => invoke('search_web', { query: `přípona souboru .${t.pripona}` })
						}
					: null,
				oddelovac,
				akceKopirovat(it.name, 'Kopírovat název'),
				akceKopirovat(it.path, 'Kopírovat celou cestu'),
				...akceHistorie(it)
			]
		});
	}

	/// Obnoví kurzor ve vstupu, aniž by cokoli přepsal.
	///
	/// Vypadá to zbytečně vedle `zaostri()`, ale řeší úplně jinou věc.
	/// Když se okno lišty teprve staví, dorazí do něj zaměření dřív, než
	/// v něm existuje načtený dokument. Výsledek je zákeřný: vstup JE
	/// `document.activeElement`, `document.hasFocus()` vrací true — a
	/// přesto se do pole nedá psát, protože rámec nemá VÝBĚR (naměřeno
	/// `getSelection().rangeCount === 0`). Blink pak nemá kam vkládat:
	/// `keydown` dorazí, `beforeinput` a `input` už ne.
	///
	/// Opakované `focus()` je proti tomu k ničemu — prvek už zaměřený
	/// je. Zabere jedině nastavení výběru.
	export function obnovKurzor() {
		if (!vstup) return;
		vstup.setSelectionRange(vstup.selectionStart ?? 0, vstup.selectionEnd ?? 0);
	}

	export function zaostri() {
		const dej = () => {
			vstup?.focus();
			vstup?.select();
		};
		dej();
		// Okno lišty dostává zaměření od Windows až po zobrazení, takže
		// první pokus může přijít dřív, než je komu ho dát. Druhý po
		// vykreslení to dorovná — psát se musí dát hned, jinak zkratka
		// nemá smysl.
		requestAnimationFrame(dej);
	}

	/// Uvést do výchozího stavu — volá se při každém vyvolání lišty.
	/// Prázdné pole, výchozí filtr, čerstvý seznam naposledy otevřených.
	export function vycisti() {
		clearTimeout(timer);
		query = '';
		hits = [];
		vybrany = 0;
		filtr = 'vse';
		busy = false;
		chyba = '';
		posledni = nactiPosledni();
		// Řádky, u kterých se ikona nesehnala, si zaslouží další pokus:
		// služba je po startu skoro minutu nemá a bez tohohle by
		// v komponentě zůstaly monogramy až do zavření aplikace.
		for (const k of [...iconState.keys()]) {
			if (!iconUrls[k]) iconState.delete(k);
		}
		nacistApps();
		nacistSvazky();
	}

	onMount(() => {
		posledni = nactiPosledni();
		nacistSvazky();
		nacistApps(true);
		zaostri();
		// Index se staví na pozadí; dokud není hotový, hlásí se to
		// a po dokončení se seznam svazků doplní sám.
		// Ptát se dál jen dokud se opravdu něco staví. Porovnávat počty
		// nešlo: svazek s trvalou chybou by se do seznamu nikdy nedostal
		// a dotaz po pipe by pak běžel každé dvě sekundy až do zavření.
		const t = setInterval(() => {
			if (!indexStav.length || indexStav.some(([, , hotovo, chyba]) => !hotovo && !chyba)) {
				nacistSvazky();
			}
		}, 2000);
		// Druhé okno (lišta vs. sekce) sdílí totéž úložiště — když si
		// tam uživatel něco otevře, seznam se má srovnat i tady.
		const sync = () => (posledni = nactiPosledni());
		window.addEventListener('storage', sync);
		return () => {
			clearInterval(t);
			clearTimeout(timer);
			window.removeEventListener('storage', sync);
		};
	});

	let stavIndexu = $derived.by(() => {
		if (!indexStav.length) return 'Index se připravuje — zatím se nemusí nic najít.';
		const stavi = indexStav
			.filter(([, , hotovo, chyba]) => !hotovo && !chyba)
			.map(([l]) => `${l}:`);
		if (stavi.length) {
			return `Prohledávám ${stavi.join(' ')} — index se ještě staví a výsledky nemusí být úplné.`;
		}
		// Svazek, který se zaindexovat nepodařilo, se z hledání tiše
		// vynechá — bez tohohle řádku by po něm nezůstala ani stopa
		// a uživatel by jen viděl, že se na tom disku nikdy nic nenajde.
		const spatne = indexStav.filter(([, , , chyba]) => chyba);
		if (spatne.length) {
			return `${spatne.map(([l]) => `${l}:`).join(' ')} se nepodařilo zaindexovat — ${spatne[0][3]}`;
		}
		return '';
	});
</script>

<!-- Klávesnice se obsluhuje na celém bloku, ne jen na vstupu: TAB má
     přepínat filtr i ve chvíli, kdy je zaměřený řádek seznamu. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fs" class:compact onkeydown={klavesa} onmousemove={() => (klavesniceVede = false)}>
	<div class="fs-bar">
		<Search size={compact ? 20 : 17} />
		<input
			bind:this={vstup}
			bind:value={query}
			oninput={napsano}
			placeholder={compact
				? 'Hledat program, soubor nebo složku…'
				: 'hledat program, soubor nebo složku…'}
			spellcheck="false"
			autocomplete="off"
		/>
		{#if busy}
			<Loader size={16} class="fs-spin" />
		{:else if vysledky.length}
			<span class="fs-count label-tech">{vysledky.length}</span>
		{/if}
	</div>

	<!-- Řádek filtrů: nápověda vlevo, přepínače vpravo. Když se
	     nevejdou, posouvá se jen jejich pruh — na „TAB" nikdy
	     nedosáhnou, protože je to samostatný sloupec. -->
	<div class="fs-tools">
		<span class="fs-tab" title="Přepnout filtr klávesou Tab">TAB</span>
		<div class="fs-chips" bind:this={chipyEl}>
			<div class="fs-chips-in">
				{#each filtry as f (f.id)}
					<button
						class="fs-chip"
						class:on={filtr === f.id}
						tabindex="-1"
						onclick={() => nastavFiltr(f.id)}
					>
						{f.label}
						<i>{pocty.get(f.id) ?? 0}</i>
					</button>
				{/each}
			</div>
		</div>
	</div>

	{#if stavIndexu}
		<p class="fs-note"><Loader size={13} class="fs-spin" />{stavIndexu}</p>
	{/if}
	{#if chyba}
		<p class="fs-err">{chyba}</p>
	{/if}

	{#if !dotaz && posledni.length && vysledky.length}
		<p class="fs-head label-tech"><Clock size={12} /> naposledy otevřené</p>
	{/if}

	{#if vysledky.length}
		<ul class="fs-list">
			{#each vysledky as it, i (it.key)}
				{@const bezi = pocetBehu(it)}
				{@const typ = it.kind === 'file' ? typSouboru(it.name) : null}
				<li>
					<button
						class="fs-row"
						class:sel={i === vyber}
						onclick={() => otevrit(it)}
						onmouseenter={() => {
							if (!klavesniceVede) vybrany = i;
						}}
						oncontextmenu={(e) => menuPolozky(e, it)}
					>
						<span class="fs-ico">
							{#if it.kind === 'app'}
								<AppIcon src={iconUrls[it.key]} name={it.name} size={18} />
							{:else if it.kind === 'dir'}
								<Folder size={17} color="var(--warn)" />
							{:else}
								{@const Ikona = IKONY_TYPU[typ.id]}
								<Ikona
									size={17}
									color={BARVY_TYPU[typ.id] ?? 'var(--text-faint)'}
									aria-label={typ.popis}
								/>
							{/if}
						</span>
						<span class="fs-main">
							<span class="fs-name">
								<span class="fs-text">
									{#each casti(it.name) as c}
										{#if c.m}<mark>{c.t}</mark>{:else}{c.t}{/if}
									{/each}
								</span>
								{#if it.system}<SystemBadge compact />{/if}
								{#if it.systemInfo}
									<SystemBadge compact level={it.systemInfo.level} title={it.systemInfo.reason} />
								{/if}
								{#if it.missing}
									<span
										class="fs-ghost"
										title="Instalační složka na disku neexistuje — po aplikaci zbyl jen záznam"
									>
										<PackageX size={13} /> chybí
									</span>
								{/if}
							</span>
							<span class="fs-path mono">{it.sub || (it.kind === 'app' ? 'program' : '')}</span>
						</span>
						{#if bezi > 0}
							<span class="fs-run" title="{bezi} běžících procesů">{bezi}</span>
						{/if}
						{#if it.size != null}
							<span class="fs-size mono">{fmtSize(it.size)}</span>
						{/if}
						{#if spousti === it.key}
							<span class="fs-enter"><Loader size={14} class="fs-spin" /></span>
						{:else if i === vyber}
							<span class="fs-enter">
								{#if it.kind === 'app'}<Play size={13} />{:else}<CornerDownLeft size={14} />{/if}
							</span>
						{/if}
					</button>
				</li>
			{/each}
		</ul>
	{:else if dotaz.length >= 2 && !busy}
		<p class="fs-empty">
			{#if filtr === 'vse'}Nic se nenašlo.{:else}V tomhle filtru nic není — TABem se přepne
				jinam.{/if}
		</p>
	{:else if dotaz}
		<p class="fs-empty">Napiš aspoň dva znaky.</p>
	{:else if !compact}
		<p class="fs-empty">
			Napiš aspoň dva znaky. Hledá se v nainstalovaných programech a v tabulce souborů NTFS, takže
			výsledky chodí okamžitě i na discích s miliony souborů.
		</p>
	{:else}
		<p class="fs-empty">Napiš aspoň dva znaky.</p>
	{/if}
</div>

<style>
	.fs {
		display: flex;
		flex-direction: column;
		min-height: 0;
		height: 100%;
	}
	.fs-bar {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 9px 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		color: var(--text-dim);
		flex: none;
	}
	.compact .fs-bar {
		padding: 14px 16px;
		border: 0;
		border-bottom: 1px solid var(--border);
		border-radius: 0;
		background: none;
	}
	.fs-bar input {
		flex: 1;
		min-width: 0;
		border: 0;
		background: none;
		color: var(--text);
		font: inherit;
		font-size: var(--fs-lg);
		outline: none;
	}
	.compact .fs-bar input {
		font-size: 1.15rem;
	}
	.fs-count {
		flex: none;
		color: var(--text-faint);
	}

	/* ── Filtry ─────────────────────────────────────────────────────
	   Vlastní řádek mezi polem a seznamem: dost odsazený, aby nesplýval
	   ani s jedním, ale ne tak, aby mezi nimi vznikla díra. */
	.fs-tools {
		display: flex;
		align-items: center;
		gap: 12px;
		flex: none;
		margin: 9px 2px 7px;
		min-width: 0;
	}
	.compact .fs-tools {
		margin: 10px 16px 8px;
	}
	.fs-tab {
		flex: none;
		font-family: var(--font-mono);
		font-size: var(--fs-3xs);
		letter-spacing: 0.1em;
		color: var(--text-faint);
		border: 1px solid var(--border);
		border-radius: 4px;
		padding: 2px 6px;
		user-select: none;
	}
	/* Pruh se posouvá vodorovně; lišta posuvníku by v tak nízkém řádku
	   byla větší než obsah, proto se schovává. */
	.fs-chips {
		flex: 1 1 auto;
		min-width: 0;
		overflow-x: auto;
		overflow-y: hidden;
		scrollbar-width: none;
		/* Náběh u levého okraje říká „pokračuje to dál". Když se
		   přepínače vejdou, jsou zarovnané doprava a na začátku je
		   prázdno — náběh tedy není vidět a nic nekazí. */
		mask-image: linear-gradient(to right, transparent 0, #000 18px);
		-webkit-mask-image: linear-gradient(to right, transparent 0, #000 18px);
		/* Aktivní přepínač nesmí skončit přesně pod náběhem. */
		scroll-padding-inline: 20px;
	}
	.fs-chips::-webkit-scrollbar {
		display: none;
	}
	/* `margin-left: auto` zarovná přepínače doprava, dokud se vejdou;
	   jakmile se nevejdou, automatický okraj vyjde nulově a pruh se
	   místo toho posouvá. Jedno pravidlo pro oba stavy. */
	.fs-chips-in {
		display: flex;
		gap: 6px;
		width: max-content;
		margin-left: auto;
		padding: 1px;
	}
	/* Tvar stejný jako přepínače v Hardwaru a Programs — jedna
	   aplikace, jeden jazyk. */
	.fs-chip {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		flex: none;
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text-dim);
		font: inherit;
		font-size: var(--fs-sm);
		padding: 3px 9px;
		cursor: pointer;
		transition:
			color 0.12s ease,
			background 0.12s ease,
			box-shadow 0.12s ease;
	}
	.fs-chip:hover {
		color: var(--text);
	}
	.fs-chip.on {
		color: var(--text);
		background: var(--surface-hover);
		box-shadow: inset 0 0 0 1px var(--border-strong);
	}
	.fs-chip i {
		font-style: normal;
		font-family: var(--font-mono);
		font-size: var(--fs-3xs);
		color: var(--text-faint);
		font-variant-numeric: tabular-nums;
	}
	.fs-chip.on i {
		color: var(--text-dim);
	}

	.fs-note,
	.fs-err,
	.fs-empty,
	.fs-head {
		display: flex;
		align-items: center;
		gap: 6px;
		margin: 0 2px 0.4rem;
		font-size: var(--fs-sm);
		color: var(--text-dim);
		flex: none;
	}
	.compact .fs-note,
	.compact .fs-err,
	.compact .fs-empty,
	.compact .fs-head {
		margin: 0 18px 0.5rem;
	}
	/* Stavba indexu je dočasná a nikoho neblokuje — jantarová říká
	   „ještě chvíli", ne „chyba". */
	.fs-note {
		color: var(--warn);
		font-size: var(--fs-xs);
	}
	.fs-head {
		color: var(--text-faint);
	}
	.fs-empty {
		display: block;
		margin-top: 0.3rem;
	}
	.fs-err {
		color: var(--danger);
	}
	.fs-list {
		list-style: none;
		margin: 0;
		padding: 0;
		overflow-y: auto;
		min-height: 0;
	}
	.compact .fs-list {
		margin: 0 0 6px;
		padding: 0 6px;
	}
	.fs-row {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		padding: 7px 10px;
		border: 0;
		border-radius: var(--radius-sm);
		background: none;
		color: var(--text);
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.fs-row.sel {
		background: var(--surface-hover);
	}
	.fs-ico {
		flex: none;
		display: grid;
		place-items: center;
		width: 20px;
		color: var(--text-faint);
	}
	.fs-main {
		display: flex;
		flex-direction: column;
		min-width: 0;
		flex: 1;
	}
	.fs-name {
		display: flex;
		align-items: center;
		gap: 5px;
		min-width: 0;
		font-size: var(--fs-lg);
	}
	.fs-text {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.fs-name mark {
		background: none;
		color: var(--ok);
		font-weight: 600;
	}
	.fs-ghost {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		flex: none;
		font-size: var(--fs-2xs);
		color: var(--warn);
		border: 1px dotted color-mix(in srgb, var(--warn) 55%, transparent);
		border-radius: 999px;
		padding: 0 6px;
	}
	.fs-path {
		font-size: var(--fs-xs);
		color: var(--text-faint);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		direction: rtl;
		text-align: left;
	}
	.fs-run {
		flex: none;
		font-family: var(--font-mono);
		font-size: var(--fs-2xs);
		color: var(--ok);
		border: 1px solid color-mix(in srgb, var(--ok) 45%, transparent);
		border-radius: 999px;
		padding: 0 6px;
		text-shadow: var(--glow-ok);
	}
	.fs-size {
		flex: none;
		font-size: var(--fs-xs);
		color: var(--text-dim);
	}
	.fs-enter {
		flex: none;
		color: var(--text-faint);
		display: grid;
		place-items: center;
	}
	:global(.fs-spin) {
		animation: fs-spin 1.1s linear infinite;
	}
	@keyframes fs-spin {
		to {
			transform: rotate(360deg);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		:global(.fs-spin) {
			animation: none;
		}
	}
</style>
