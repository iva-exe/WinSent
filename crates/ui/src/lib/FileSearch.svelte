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
	import { nacti as nactiPosledni, zapamatuj, zapomen } from '$lib/recent.js';
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
			svazky = indexStav.filter(([, , hotovo]) => hotovo).map(([l]) => l);
		} catch {
			svazky = [];
		}
	}

	// Inventář a procesy se drží v paměti a obnovují jen občas: seznam
	// aplikací se mění po instalaci, ne mezi dvěma stisky klávesy.
	let nactenoAppsMs = 0;
	async function nacistApps(force = false) {
		if (!force && Date.now() - nactenoAppsMs < 60_000) return;
		nactenoAppsMs = Date.now();
		try {
			apps = await invoke('query_apps');
		} catch {
			/* služba mimo — hledají se aspoň soubory */
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

	async function fetchIcon(key) {
		if (!key) return;
		const st = iconState.get(key) ?? 0;
		if (st === 'done' || st >= 3) return;
		iconState.set(key, st + 1);
		try {
			const icon = await invoke('query_icon', { identityKey: key });
			if (icon) {
				iconUrls[key] = rgbaToUrl(icon);
				iconState.set(key, 'done');
			}
		} catch {
			/* služba mimo — zkusí se příště */
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
			const davky = await Promise.all(
				svazky.map((l) =>
					invoke('search_files', { letter: l, query: q, limit: LIMIT }).catch(() => [])
				)
			);
			if (muj !== beh) return;
			// Složky napřed, pak podle délky cesty: co je blíž kořeni,
			// bývá to hledané. Uvnitř abecedně, ať pořadí neposkakuje.
			hits = davky
				.flat()
				.sort((a, b) => {
					const da = (a.attrs & ATTR_DIR) !== 0;
					const db = (b.attrs & ATTR_DIR) !== 0;
					if (da !== db) return da ? -1 : 1;
					const la = a.path.split(/[\\/]/).length;
					const lb = b.path.split(/[\\/]/).length;
					if (la !== lb) return la - lb;
					return a.name.localeCompare(b.name, 'cs');
				})
				.slice(0, LIMIT);
			chyba = '';
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

	/// Skóre shody jména aplikace s dotazem: 0 = začíná jím,
	/// 1 = začíná jím některé slovo, 2 = obsahuje ho, −1 = neshoda.
	function skore(jmeno, q) {
		const i = jmeno.indexOf(q);
		if (i < 0) return -1;
		if (i === 0) return 0;
		return /[\s\-_.(]/.test(jmeno[i - 1]) ? 1 : 2;
	}

	/// Aplikace odpovídající dotazu, seřazené podle shody. Filtruje se
	/// v paměti — inventář je pár set položek, dotaz přes pipe by byl
	/// pomalejší než celé porovnání.
	let appHits = $derived.by(() => {
		if (dotaz.length < 2) return [];
		const q = dotaz.toLowerCase();
		return apps
			.map((a) => ({ a, s: skore((a.display_name ?? '').toLowerCase(), q) }))
			.filter((x) => x.s >= 0)
			.sort(
				(x, y) =>
					x.s - y.s ||
					x.a.display_name.length - y.a.display_name.length ||
					x.a.display_name.localeCompare(y.a.display_name, 'cs')
			)
			.map((x) => polozkaApp(x.a));
	});

	// ── Jednotná položka seznamu ─────────────────────────────────────
	// Aplikace, soubory, složky i „naposledy otevřené" se kreslí týmž
	// řádkem. Jinak by se tři skoro stejné šablony rozešly v detailech.
	function polozkaApp(a) {
		return {
			kind: 'app',
			key: `app:${a.identity_key}`,
			name: a.display_name,
			sub: a.publisher ?? '',
			path: '',
			identity_key: a.identity_key,
			attrs: 0,
			disk: '',
			size: null,
			missing: !!a.missing_install,
			system: isSystemApp(a),
			systemInfo: null
		};
	}

	function polozkaSoubor(h) {
		const dir = (h.attrs & ATTR_DIR) !== 0;
		return {
			kind: dir ? 'dir' : 'file',
			key: `f:${h.path}`,
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
			sub: r.sub ?? '',
			path: r.path ?? '',
			identity_key: r.identity_key ?? '',
			attrs: r.attrs ?? 0,
			disk: r.disk ?? '',
			size: null,
			missing: false,
			system:
				r.kind === 'app' &&
				isSystemApp({ identity_key: r.identity_key, display_name: r.name, publisher: r.sub }),
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

	/// Co se opravdu kreslí. Ve „Vše" se aplikace ořezávají, ať
	/// nevytlačí soubory z první obrazovky.
	let vysledky = $derived.by(() => {
		const v = vse.filter((it) => projde(it, filtr));
		if (filtr !== 'vse') return v;
		const a = v.filter((it) => it.kind === 'app').slice(0, APPS_VE_VSEM);
		return [...a, ...v.filter((it) => it.kind !== 'app')];
	});

	let pocty = $derived.by(() => {
		const m = new Map();
		for (const f of filtry) m.set(f.id, 0);
		for (const it of vse) {
			m.set('vse', (m.get('vse') ?? 0) + 1);
			m.set(it.kind, (m.get(it.kind) ?? 0) + 1);
			if (it.disk) m.set(`disk:${it.disk}`, (m.get(`disk:${it.disk}`) ?? 0) + 1);
		}
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
			if (it.kind === 'app') fetchIcon(it.identity_key);
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
		return i > 0 ? p.slice(0, i) : p;
	}

	/// Zvýrazní část názvu, která odpovídá dotazu. Bez toho není poznat,
	/// proč se řádek objevil.
	function casti(text) {
		if (!dotaz) return [{ t: text, m: false }];
		const i = text.toLowerCase().indexOf(dotaz.toLowerCase());
		if (i < 0) return [{ t: text, m: false }];
		return [
			{ t: text.slice(0, i), m: false },
			{ t: text.slice(i, i + dotaz.length), m: true },
			{ t: text.slice(i + dotaz.length), m: false }
		].filter((c) => c.t);
	}

	// ── Akce ─────────────────────────────────────────────────────────
	async function otevrit(it) {
		try {
			if (it.kind === 'app') {
				await invoke('launch_app', { identityKey: it.identity_key, displayName: it.name });
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
		}
		onhotovo();
	}

	function klavesa(e) {
		if (e.key === 'Tab') {
			e.preventDefault();
			posunFiltr(e.shiftKey ? -1 : 1);
		} else if (e.key === 'ArrowDown') {
			e.preventDefault();
			vybrany = Math.min(vyber + 1, vysledky.length - 1);
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
	function doHledu() {
		queueMicrotask(() => {
			document.querySelector('.fs-row.sel')?.scrollIntoView({ block: 'nearest' });
		});
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
					it.nedavne
						? { label: 'Vymazat historii', icon: 'trash', run: () => (posledni = zapomen()) }
						: null
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
				it.nedavne
					? { label: 'Vymazat historii', icon: 'trash', run: () => (posledni = zapomen()) }
					: null
			]
		});
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
		const t = setInterval(() => {
			if (svazky.length < indexStav.length || !indexStav.length) nacistSvazky();
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
		const chybi = indexStav.filter(([, , hotovo]) => !hotovo).map(([l]) => `${l}:`);
		if (!chybi.length) return '';
		return `Prohledávám ${chybi.join(' ')} — index se ještě staví a výsledky nemusí být úplné.`;
	});
</script>

<!-- Klávesnice se obsluhuje na celém bloku, ne jen na vstupu: TAB má
     přepínat filtr i ve chvíli, kdy je zaměřený řádek seznamu. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fs" class:compact onkeydown={klavesa}>
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
						onmouseenter={() => (vybrany = i)}
						oncontextmenu={(e) => menuPolozky(e, it)}
					>
						<span class="fs-ico">
							{#if it.kind === 'app'}
								<AppIcon src={iconUrls[it.identity_key]} name={it.name} size={18} />
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
						{#if i === vyber}
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
