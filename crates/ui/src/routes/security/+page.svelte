<script>
	// Security (v9, SPEC kap. 13) — stav ochrany + oprávnění aplikací.
	//
	// Dvě části:
	//  1. „Jsem chráněný?" — antivirus, firewall, Secure Boot, TPM,
	//     UAC, šifrování disků. Fakta, žádné verdikty.
	//  2. Oprávnění: kdo má přístup ke kameře, mikrofonu, poloze —
	//     a kdo je používá PRÁVĚ TEĎ (živá tečka).
	//
	// Nejdůležitější pravidlo (SPEC 13.4): Windows NENÍ macOS. U balené
	// aplikace Deny systém tvrdě vynutí — zelená. U klasické Win32
	// aplikace je Deny jen deklarace, kterou jde obejít — jantarová
	// s vysvětlením. Zelená NIKDY tam, kde vynucení není: falešný
	// pocit ochrany je horší než žádný.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { openMenu, akceKopirovat, akceOtevritUmisteni, oddelovac } from '$lib/itemmenu.svelte.js';
	import {
		Camera,
		ChevronRight,
		FileLock2,
		Flame,
		FolderOpen,
		HardDrive,
		Mic,
		MapPin,
		MonitorUp,
		RefreshCw,
		Shield,
		ShieldCheck,
		ShieldAlert,
		ShieldOff,
		TriangleAlert,
		UserCheck
	} from 'lucide-svelte';

	let report = $state(null);
	let loadError = $state('');

	async function load() {
		try {
			report = await invoke('query_security');
			loadError = '';
		} catch (e) {
			loadError = String(e);
		}
	}

	onMount(() => {
		load();
		loadTotals();
		// Oprávnění jsou levná (registr) a živá tečka má žít; stav
		// ochrany si služba cachuje na 30 s sama.
		const t = setInterval(load, 3000);
		return () => clearInterval(t);
	});

	// ── Stav ochrany jako řádky: (ikona, název, stav, detail, tón) ──
	let protectionRows = $derived.by(() => {
		if (!report) return [];
		const p = report.protection;
		const rows = [];

		// Antivirus.
		const av = p.av.filter(([n]) => n);
		// Osiřelé registrace se nepočítají za ochranu. Security Center
		// po odinstalaci registraci nemusí uklidit, takže tam roky visí
		// antivirus, který na počítači není — a tvrdí, že běží.
		const live = av.filter(([, , , leftover]) => !leftover);
		const gone = av.filter(([, , , leftover]) => leftover);
		if (live.length) {
			for (const [name, enabled, fresh] of live) {
				const d = p.defender;
				const extra =
					d && name.toLowerCase().includes('defender')
						? d[1] != null
							? ` · definice ${d[1] === 0 ? 'dnešní' : `staré ${d[1]} d`}` +
								(d[2] != null ? ` · rychlý sken před ${d[2]} d` : '')
							: ''
						: '';
				rows.push({
					icon: ShieldCheck,
					name,
					state: enabled ? 'běží' : 'vypnutý',
					detail: (enabled ? (fresh ? 'aktuální definice' : 'ZASTARALÉ definice') : '') + extra,
					tone: enabled && fresh ? 'ok' : 'warn'
				});
			}
		} else {
			rows.push({
				icon: ShieldCheck,
				name: 'Antivirus',
				state: 'nenalezen',
				detail: 'Security Center žádný nehlásí',
				tone: 'warn'
			});
		}
		// Dva současně běžící antiviry si navzájem berou soubory pod
		// rukama — je to informace, ne příkaz: který z nich odejde, je
		// rozhodnutí uživatele.
		const active = live.filter(([, enabled]) => enabled).map(([n]) => n);
		if (active.length > 1) {
			rows.push({
				icon: ShieldAlert,
				name: 'Dva antiviry naráz',
				state: `${active.length} současně`,
				detail: `${active.join(' + ')} — oba hlídají soubory v reálném čase`,
				tone: 'warn'
			});
		}
		// Zbytek po odinstalovaném antiviru: ne poplach, ale vysvětlení,
		// proč ho Windows možná pořád někde uvádějí.
		for (const [name] of gone) {
			rows.push({
				icon: ShieldOff,
				name,
				state: 'zbytek po odinstalaci',
				detail: 'registrace ve Windows zůstala, program na disku není — nechrání nic',
				tone: 'dim'
			});
		}

		// Firewall — tři profily.
		const fw = [
			['doména', p.fw_domain],
			['privátní', p.fw_private],
			['veřejná', p.fw_public]
		];
		const fwOff = fw.filter(([, v]) => v === false).map(([n]) => n);
		rows.push({
			icon: Flame,
			name: 'Firewall',
			state: fwOff.length === 0 ? 'zapnutý' : `vypnutý (${fwOff.join(', ')})`,
			detail: fw.map(([n, v]) => `${n}: ${v == null ? '—' : v ? 'ano' : 'NE'}`).join(' · '),
			tone: fwOff.length === 0 ? 'ok' : 'warn'
		});

		// Secure Boot: None = legacy BIOS, kde neexistuje.
		rows.push({
			icon: Shield,
			name: 'Secure Boot',
			state: p.secure_boot == null ? 'není k dispozici' : p.secure_boot ? 'zapnutý' : 'vypnutý',
			detail:
				p.secure_boot == null
					? 'stroj bootuje přes legacy BIOS — Secure Boot na něm neexistuje'
					: '',
			tone: p.secure_boot == null ? 'dim' : p.secure_boot ? 'ok' : 'warn'
		});

		// TPM. Prázdná verze specifikace = čip ve stromu zařízení je,
		// ale WMI o něm mlčí (typicky bez práv). „Nenalezen" by v té
		// chvíli bylo tvrzení o hardwaru, které neplyne z ničeho.
		rows.push({
			icon: FileLock2,
			name: 'TPM',
			state: p.tpm == null ? 'nenalezen' : p.tpm[1] ? (p.tpm[0] ? 'zapnutý' : 'vypnutý') : 'nezjištěno',
			detail: p.tpm
				? p.tpm[1]
					? `specifikace ${p.tpm[1]}`
					: 'čip v počítači je, ale Windows o něm přes WMI nic neřekly'
				: 've stromu zařízení žádný čip není',
			tone: p.tpm?.[0] && p.tpm?.[1] ? 'ok' : 'dim'
		});

		// UAC.
		rows.push({
			icon: UserCheck,
			name: 'Řízení uživatelských účtů (UAC)',
			state: p.uac_enabled ? 'zapnuté' : 'VYPNUTÉ',
			detail: p.uac_enabled
				? p.uac_admin_prompt === 0
					? 'výzvy jsou potlačené — správce se nikdy neptá'
					: 'výzva při změnách vyžadujících správce'
				: 'programy mohou měnit systém bez ptaní',
			tone: p.uac_enabled && p.uac_admin_prompt !== 0 ? 'ok' : 'warn'
		});

		// Šifrování disků.
		if (p.encryption.length) {
			const on = p.encryption.filter(([, s]) => s === 1).map(([l]) => l);
			const off = p.encryption.filter(([, s]) => s === 0).map(([l]) => l);
			rows.push({
				icon: HardDrive,
				name: 'Šifrování disku (BitLocker)',
				state: on.length ? `zapnuté (${on.join(', ')})` : 'vypnuté',
				detail: off.length ? `nešifrované svazky: ${off.join(', ')}` : '',
				// Nešifrovaný disk je fakt, ne poplach — spousta strojů
				// BitLocker vědomě nepoužívá.
				tone: on.length ? 'ok' : 'dim'
			});
		}

		// Stav aktualizací. Neinstaluje se odsud nic — je to informace
		// o tom, jestli si systém záplaty vůbec bere. Čas poslední
		// instalace novější Windows v registru nevedou, a tak se místo
		// vymyšleného data přizná, že se nezjistil.
		const o = p.os;
		if (o?.build) {
			const svcOff = o.update_service_start === 4 || o.update_disabled_by_policy;
			const days =
				o.update_last_search != null
					? Math.floor((Date.now() / 1000 - o.update_last_search) / 86400)
					: null;
			rows.push({
				icon: RefreshCw,
				name: 'Aktualizace Windows',
				state: svcOff
					? 'VYPNUTÉ'
					: days == null
						? 'nezjištěno'
						: days === 0
							? 'kontrolováno dnes'
							: `kontrolováno před ${days} dny`,
				detail: [
					o.update_disabled_by_policy ? 'zakázané zásadou' : null,
					o.update_service_start === 4 ? 'služba zakázaná' : null,
					o.update_last_install ? `instalováno ${fmtDay(o.update_last_install)}` : null
				]
					.filter(Boolean)
					.join(' · '),
				tone: svcOff ? 'warn' : days != null && days > 30 ? 'warn' : days == null ? 'dim' : 'ok'
			});
		}
		return rows;
	});

	// Verze systému do hlavičky — nejzákladnější údaj o stroji,
	// který v aplikaci dosud nikde nebyl.
	const osLine = $derived.by(() => {
		const o = report?.protection?.os;
		if (!o?.build) return null;
		return `${o.product}${o.display_version ? ' ' + o.display_version : ''} · ${o.arch} · sestavení ${o.build}.${o.ubr}`;
	});

	function fmtDay(t) {
		return new Date(t * 1000).toLocaleDateString('cs-CZ');
	}

	// Vysvětlivky k dlaždicím: co to je a proč na tom záleží.
	//
	// Bez nich je „TPM: zapnutý" údaj, kterému laik nerozumí — a přesně
	// takový člověk je cílem téhle sekce. Klíčem je název dlaždice;
	// antivirus se jmenuje podle výrobce, ten se dohledá zvlášť.
	const explain = {
		Firewall:
			'Hlídá, co se z internetu smí dostat do počítače. Zapnutý má být na všech třech profilech.',
		'Secure Boot':
			'Při startu pouští jen podepsaný systém. Brání tomu, aby se něco zavrtalo pod Windows.',
		TPM: 'Čip, ve kterém jsou uložené klíče k šifrování disku. Bez něj BitLocker chce heslo při každém startu.',
		'Řízení uživatelských účtů (UAC)':
			'Ptá se, než program změní systém. Vypnuté UAC znamená, že se ptát nikdo nebude.',
		'Šifrování disku (BitLocker)':
			'Bez šifrování si obsah disku přečte kdokoliv, kdo ho vyndá z počítače. Na stolním počítači doma to spousta lidí vědomě nemá.',
		Antivirus: 'Žádný antivirus systém nehlásí — Windows Defender bývá vypnutý, když je nainstalovaný jiný.',
		'Aktualizace Windows':
			'Záplaty zavírají díry, kterými se do systému dostávají útoky. Winsent odsud nic neinstaluje ani nemění — jen ukazuje, kdy systém naposledy kontroloval.'
	};

	// Vysvětlivka k antiviru je společná bez ohledu na jeho jméno.
	function explainFor(name) {
		if (explain[name]) return explain[name];
		return 'Sleduje soubory a procesy a zasahuje, když najde něco škodlivého. Důležité je hlavně to, že běží a má aktuální definice.';
	}

	// ── Oprávnění seskupená podle schopnosti ──
	// Kontextové menu oprávnění.
	//
	// Vyhledává se jméno aplikace, ne cesta k .exe — cesta je pro
	// vyhledávač šum a navíc nese jméno uživatele. Když je jméno obecné
	// („javaw", „Update"), přidá `openMenu` cestu jako druhý kandidát
	// až po ní, takže se použije jen když samotné jméno nestačí.
	function menuOpravneni(e, p, g) {
		const jmeno = p.app_name || p.app;
		const cesta = p.app?.includes('\\') ? p.app : '';
		openMenu(e, {
			title: jmeno,
			subtitle: `${g.label} · ${p.allow ? 'povoleno' : 'odepřeno'}`,
			hledat: [jmeno],
			kontext: 'aplikace',
			items: [
				{
					label: 'Nastavení soukromí ve Windows',
					icon: 'shield',
					hint: g.label,
					// Winsent oprávnění nepřepíná — otevře stránku, kde to
					// udělá uživatel sám (SPEC 13.4: my ukazujeme, on mačká).
					run: () => invoke('open_settings_page', { page: `privacy-${p.capability}` })
				},
				cesta ? akceOtevritUmisteni(cesta) : null,
				oddelovac,
				akceKopirovat(jmeno),
				cesta ? akceKopirovat(cesta, 'Kopírovat cestu') : null
			]
		});
	}

	const CAPS = {
		webcam: { label: 'Kamera', icon: Camera },
		microphone: { label: 'Mikrofon', icon: Mic },
		location: { label: 'Poloha', icon: MapPin },
		screenCapture: { label: 'Snímání obrazovky', icon: MonitorUp },
		broadFileSystemAccess: { label: 'Přístup k celému disku', icon: FolderOpen },
		documentsLibrary: { label: 'Dokumenty', icon: FolderOpen },
		picturesLibrary: { label: 'Obrázky', icon: FolderOpen },
		videosLibrary: { label: 'Videa', icon: FolderOpen },
		musicLibrary: { label: 'Hudba', icon: FolderOpen },
		downloadsFolder: { label: 'Stažené soubory', icon: FolderOpen },
		contacts: { label: 'Kontakty', icon: UserCheck },
		appointments: { label: 'Kalendář', icon: UserCheck },
		email: { label: 'E-mail', icon: UserCheck },
		phoneCall: { label: 'Telefonování', icon: UserCheck }
	};
	// Pořadí: nejcitlivější první.
	const CAP_ORDER = Object.keys(CAPS);

	// Rozbalené aplikace se starými verzemi (klíč = kategorie + aplikace).
	let openVersions = $state(new Set());
	function toggleVersions(key) {
		const s = new Set(openVersions);
		if (s.has(key)) s.delete(key);
		else s.add(key);
		openVersions = s;
	}

	let permGroups = $derived.by(() => {
		if (!report) return [];
		const by = new Map();
		for (const p of report.permissions) {
			if (!by.has(p.capability)) by.set(p.capability, []);
			by.get(p.capability).push(p);
		}
		return CAP_ORDER.filter((c) => by.has(c)).map((c) => {
			// Verze téže aplikace do jednoho řádku.
			//
			// Windows si oprávnění pamatují ke KAŽDÉ cestě zvlášť, takže
			// aplikace, která se instaluje do složky s číslem verze, si
			// za roky nasbírá vlastní záznam pro každou verzi — Discord
			// jich má na mikrofon 141, nejstarší z roku 2020. Všechny
			// říkají totéž a zajímá jen ta, kterou uživatel používá teď.
			// Staré se neztrácejí, jen se schovají pod rozklik.
			const byApp = new Map();
			for (const p of by.get(c)) {
				const k = p.group_key ?? p.app;
				if (!byApp.has(k)) byApp.set(k, []);
				byApp.get(k).push(p);
			}
			const items = [...byApp.entries()].map(([key, versions]) => {
				// Nahoru ta verze, která se používá teď, jinak naposledy
				// použitá — ta je pro uživatele ta „aktuální".
				versions.sort(
					(a, b) => b.in_use - a.in_use || (b.last_used ?? 0) - (a.last_used ?? 0)
				);
				const head = versions[0];
				return {
					key,
					...head,
					versions,
					// Liší se některá stará verze povolením? Pak se to musí
					// říct — jinak by rozklik ukázal něco jiného než řádek.
					mixed: versions.some((v) => v.allow !== head.allow),
					// Aplikace „se používá", i kdyby to hlásila jiná verze.
					in_use: versions.some((v) => v.in_use)
				};
			});
			items.sort(
				(a, b) =>
					b.in_use - a.in_use ||
					(b.last_used ?? 0) - (a.last_used ?? 0) ||
					a.app_name.localeCompare(b.app_name, 'cs')
			);
			return {
				cap: c,
				...CAPS[c],
				items,
				inUse: items.filter((i) => i.in_use && i.allow),
				allowed: items.filter((i) => i.allow).length
			};
		});
	});

	// Počty do hlavičky: aplikací, ne záznamů. „254 oprávnění" je
	// pravda o registru, ale ne o tom, co uživatel na stránce vidí.
	let permCount = $derived(permGroups.reduce((n, g) => n + g.items.length, 0));

	let liveNow = $derived(
		(report?.permissions ?? []).filter(
			(p) => p.in_use && p.allow && (p.capability === 'webcam' || p.capability === 'microphone')
		)
	);

	// Kolik času aplikace schopnost držela za posledních 30 dní.
	//
	// Načítá se JEDNÍM dotazem pro všechny řádky. Dřív to bylo schované
	// za tlačítkem „kolik času?", protože každý řádek stál vlastní dotaz.
	// Jenže to znamenalo, že se uživatel musel proklikat k tomu
	// nejzajímavějšímu údaji na celé stránce.
	let totals = $state(new Map());

	async function loadTotals() {
		try {
			const rows = await invoke('query_perm_use_totals', { days: 30 });
			const m = new Map();
			for (const [app, cap, secs] of rows) m.set(`${cap}|${app}`, secs);
			totals = m;
		} catch {
			/* historie je bonus — bez ní řádek pořád dává smysl */
		}
	}

	// Součet za aplikaci. Čas se počítá každé verzi zvlášť, ale uživatele
	// zajímá součet za aplikaci jako celek.
	function totalFor(cap, group) {
		let s = 0;
		let known = false;
		for (const v of group.versions ?? [group]) {
			const t = totals.get(`${cap}|${v.app}`);
			if (t != null) {
				s += t;
				known = true;
			}
		}
		return known ? s : null;
	}

	// Rozbalené kategorie. Po startu jsou zavřené všechny — čtrnáct
	// kategorií se sedmdesáti řádky naráz je zeď, ve které se nedá nic
	// najít; hlavičky samy o sobě říkají, kolik čeho je.
	let openCaps = $state(new Set());
	function toggleCap(cap) {
		const s = new Set(openCaps);
		if (s.has(cap)) s.delete(cap);
		else s.add(cap);
		openCaps = s;
	}
	function capOpen(cap) {
		return openCaps.has(cap);
	}

	// Doba trvání lidsky. Vteřiny se u „držel mikrofon" nikoho neptají.
	function fmtDur(s) {
		if (s == null) return null;
		if (s < 60) return `${Math.max(0, Math.round(s))} s`;
		const h = Math.floor(s / 3600);
		const m = Math.round((s % 3600) / 60);
		return h ? `${h} h ${m} min` : `${m} min`;
	}

	function fmtWhen(ts) {
		if (!ts) return null;
		const d = new Date(ts * 1000);
		const today = new Date();
		const sameDay = d.toDateString() === today.toDateString();
		const t = d.toLocaleTimeString('cs-CZ', { hour: '2-digit', minute: '2-digit' });
		if (sameDay) return `dnes ${t}`;
		const yesterday = new Date(today.getTime() - 86400e3);
		if (d.toDateString() === yesterday.toDateString()) return `včera ${t}`;
		return d.toLocaleDateString('cs-CZ') + ' ' + t;
	}
</script>

<div class="page">
	<header class="head">
		<h1>Security</h1>
		<span class="label-tech">
			{permCount} aplikací · {permGroups.length} kategorií
		</span>
		{#if osLine}
			<span class="os-line label-tech">{osLine}</span>
		{/if}
		{#if liveNow.length}
			<span class="live-warn">
				<span class="live-dot"></span>
				{liveNow.map((l) => l.app_name).join(', ')}
				{liveNow.length === 1 ? 'používá' : 'používají'}
				{liveNow[0].capability === 'webcam' ? 'kameru' : 'mikrofon'} právě teď
			</span>
		{/if}
	</header>

	{#if loadError}
		<p class="empty">Nelze načíst stav zabezpečení: {loadError}</p>
	{:else if report}
		<div class="body">
			<!-- ── 1. Jsem chráněný? ── -->
			<h2 class="sect"><ShieldCheck size={16} /> Stav ochrany</h2>
			<!-- Dlaždice, ne řádky: tohle je pár údajů, u kterých má být
			     stav vidět na první pohled a hned u něj vysvětlení, co to
			     pro uživatele znamená. Seznam řádků je až pro oprávnění,
			     kterých jsou desítky. -->
			<div class="tiles">
				{#each protectionRows as r, ri (ri + ':' + r.name)}
					<article class="tile {r.tone}">
						<div class="t-top">
							<span class="t-ico"><r.icon size={18} /></span>
							<span class="t-name">{r.name}</span>
						</div>
						<span class="t-state">
							{#if r.tone === 'warn'}<TriangleAlert size={15} />{/if}
							{r.state}
						</span>
						{#if r.detail}<p class="t-detail">{r.detail}</p>{/if}
						{#if explainFor(r.name)}
							<!-- Vysvětlivka: co to je a proč na tom záleží. Bez ní
							     je „TPM: zapnutý" údaj, kterému laik nerozumí. -->
							<p class="t-explain">{explainFor(r.name)}</p>
						{/if}
					</article>
				{/each}
			</div>

			<!-- ── 2. Oprávnění aplikací ── -->
			<div class="split">
				<h2><Camera size={17} /> Kdo má přístup k čemu</h2>
				<p>
					Oprávnění po kategoriích — kategorie jsou zavřené, klikni na ni pro rozbalení. U každé aplikace je
					vidět, kdy schopnost použila naposledy a kolik času ji držela za posledních 30 dnů.
				</p>
			</div>
			{#each permGroups as g (g.cap)}
				{@const capIsOpen = capOpen(g.cap)}
				<!-- Kategorie se rozbaluje. Deset kategorií se sedmdesáti
				     řádky naráz je zeď, ve které se nedá nic najít. -->
				<button class="cap-head" class:on={capIsOpen} onclick={() => toggleCap(g.cap)}>
					<ChevronRight class="cap-caret" size={15} strokeWidth={2.25} />
					<g.icon size={17} />
					<span class="cap-label">{g.label}</span>
					<span class="cap-n">{g.items.length} aplikací · {g.allowed} s přístupem</span>
					{#if g.inUse.length}
						<span class="sect-live"><span class="live-dot"></span>používá se právě teď</span>
					{/if}
				</button>
				{#if capIsOpen}
					<!-- Seznam je vizuálně vnořený do kategorie nad ním: odsazený
					     z obou stran a s linkou vlevo. Bez toho karty splývaly
					     s hlavičkou a nebylo poznat, kde kategorie končí. -->
					<div class="cap-body">
				{#each g.items as p (g.cap + p.key)}
					{@const okey = g.cap + p.key}
					{@const open = openVersions.has(okey)}
					<article class="item slim" class:live={p.in_use && p.allow} oncontextmenu={(e) => menuOpravneni(e, p, g)}>
						<div class="info">
							<h3 class="perm-name">
								{p.app_name}
								{#if p.in_use && p.allow}
									<span class="live-dot" title="používá právě teď"></span>
								{/if}
								{#if p.versions.length > 1}
									<!-- Staré verze se nemažou ani neschovávají před
									     uživatelem — jen ustoupí za rozklik. -->
									<button
										class="vers"
										class:mixed={p.mixed}
										onclick={() => toggleVersions(okey)}
										title={p.mixed
											? 'Starší verze mají jiné nastavení — rozklikni'
											: 'Starší verze téže aplikace'}
									>
										{p.versions.length - 1} starších verzí
										{#if p.mixed}· liší se{/if}
										<ChevronRight class="vers-caret" size={12} strokeWidth={2.25} />
									</button>
								{/if}
							</h3>
							<p class="vendor mono">{p.app}</p>
						</div>
						<div class="side">
							<!-- Stav je řádek textu s barvou, ne velký barevný
							     chip. Chip se u dlouhého popisu ("odepřeno —
							     nevynuceno") netrhal a tlačil se do jména
							     aplikace vlevo. -->
							{#if p.in_use && p.allow}
								<span class="p-state live"><span class="live-dot"></span>používá právě teď</span>
							{:else if p.allow}
								<span class="p-state">povoleno</span>
							{:else if p.enforced}
								<!-- Balená aplikace: Windows Deny VYNUTÍ — jediné
								     místo, kde smí být zelená. -->
								<span class="p-state ok"><Shield size={12} /> zablokováno</span>
							{:else}
								<!-- Win32: deklarace bez vynucení. Nikdy zelená. -->
								<span
									class="p-state warn"
									title="Klasická aplikace se ke kameře či mikrofonu může dostat i mimo tohle nastavení (přes ovladač). Windows to na rozdíl od balených aplikací tvrdě nevynucují."
								>
									<TriangleAlert size={12} /> odepřeno · nevynuceno
								</span>
							{/if}
							<dl class="meta">
								<div>
									<dt>Naposledy</dt>
									<dd>{p.in_use ? 'právě teď' : (fmtWhen(p.last_used) ?? 'nikdy')}</dd>
								</div>
								<div>
									<dt>Posledních 30 dnů</dt>
									<dd class:zero={totalFor(g.cap, p) === 0}>
										{totalFor(g.cap, p) == null
											? '—'
											: totalFor(g.cap, p) === 0
												? 'nepoužito'
												: fmtDur(totalFor(g.cap, p))}
									</dd>
								</div>
							</dl>
						</div>
					</article>
					{#if open}
						<!-- Klíč nese i pořadí: dvakrát stejný klíč ve {#each}
						     je v produkci tvrdá chyba, která zabije překreslování
						     celé stránky. -->
						{#each p.versions.slice(1) as v, vi (vi + ':' + v.app)}
							<article class="item slim ver-row">
								<div class="info">
									<p class="vendor mono">{v.app}</p>
								</div>
								<div class="side">
									<span class="p-state">
										{v.allow ? "povoleno" : "odepřeno"}{#if v.last_used}&nbsp;· naposledy
											{fmtWhen(v.last_used)}{/if}
									</span>
								</div>
							</article>
						{/each}
					{/if}
				{/each}
					</div>
				{/if}
			{/each}

			<p class="note">
				Stav ochrany i oprávnění jsou fakta přečtená ze systému — Winsent nic nehodnotí ani
				neskenuje. U klasických aplikací Windows „odepřeno" tvrdě nevynucují; jantarová barva
				říká přesně tohle, protože falešný pocit ochrany je horší než žádný.
			</p>
		</div>
	{/if}
</div>

<style>
	/* Oddělovač mezi dlaždicemi a seznamem oprávnění — jsou to dvě
	   různé věci a bez hranice splývaly do jedné dlouhé kaše. */
	.split {
		margin: 26px 0 14px;
		padding-top: 20px;
		border-top: 1px solid var(--border);
	}
	.split h2 {
		display: flex;
		align-items: center;
		gap: 9px;
		margin: 0;
		font-size: 1.02rem;
		font-weight: 600;
	}
	.split p {
		margin: 5px 0 0;
		font-size: var(--fs-md);
		color: var(--text-faint);
		line-height: 1.5;
	}
	/* Hlavička kategorie = rozbalovací pruh. */
	/* Tělo kategorie. Odsazení z obou stran a linka vlevo říkají, že
	   karty patří pod hlavičku nad nimi — bez toho to byl jeden
	   nekonečný sloupec, ve kterém kategorie nešly rozeznat. */
	.cap-body {
		margin: 0 12px 18px 14px;
		padding-left: 16px;
		border-left: 2px solid var(--border);
	}
	.cap-head {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		margin-bottom: 6px;
		padding: 11px 13px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		color: var(--text-dim);
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.cap-head:hover {
		background: var(--surface-hover);
		color: var(--text);
	}
	.cap-head.on {
		color: var(--text);
		box-shadow: inset 0 0 0 1px var(--border-strong);
	}
	:global(.cap-caret) {
		transition: transform 0.15s ease;
		flex: none;
	}
	.cap-head.on :global(.cap-caret) {
		transform: rotate(90deg);
	}
	.cap-label {
		font-size: 0.94rem;
		font-weight: 600;
	}
	.cap-n {
		margin-left: auto;
		font-family: var(--font-mono);
		font-size: var(--fs-xs);
		color: var(--text-faint);
		font-variant-numeric: tabular-nums;
	}
	/* Mřížka popisek → hodnota vpravo na řádku. Popisky jsou v jednom
	   jazyce (mono verzálky) bez ohledu na to, že hodnoty jsou různé
	   typy — čas, doba, stav. */
	.meta {
		display: grid;
		grid-template-columns: repeat(2, minmax(96px, auto));
		gap: 2px 18px;
		margin: 0;
		text-align: right;
	}
	.meta dt {
		font-family: var(--font-mono);
		font-size: var(--fs-3xs);
		letter-spacing: 0.05em;
		text-transform: uppercase;
		color: var(--text-faint);
	}
	.meta dd {
		margin: 1px 0 0;
		font-size: var(--fs-md);
		color: var(--text);
		font-variant-numeric: tabular-nums;
	}
	.meta dd.zero {
		color: var(--text-faint);
	}
	/* Dlaždice stavu ochrany. Šířka se přizpůsobí oknu; obsah je
	   pokaždé stejně stavěný, aby se očima dalo skákat po stavech. */
	.tiles {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
		gap: 10px;
		margin-bottom: 6px;
	}
	.tile {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 18px 20px 20px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
	}
	/* Barvu nese ikona, ne rám. Barevný proužek u každé dlaždice dělal
	   z přehledu pruhovanou tabulku; ikona nese stav a zbytek zůstává
	   klidný. */
	.t-top {
		display: flex;
		align-items: center;
		gap: 10px;
		color: var(--text-dim);
	}
	.t-ico {
		display: grid;
		place-items: center;
		width: 34px;
		height: 34px;
		border-radius: 10px;
		background: var(--surface-hover);
		color: var(--text-dim);
		flex: none;
	}
	.tile.ok .t-ico {
		color: var(--ok);
		background: color-mix(in srgb, var(--ok) 14%, transparent);
	}
	.tile.warn .t-ico {
		color: var(--warn);
		background: color-mix(in srgb, var(--warn) 14%, transparent);
	}
	.t-name {
		font-size: var(--fs-lg);
		font-family: var(--font-mono);
		letter-spacing: 0.03em;
		text-transform: uppercase;
	}
	.t-state {
		display: flex;
		align-items: center;
		gap: 7px;
		margin-top: 2px;
		font-size: 1.28rem;
		font-weight: 600;
		line-height: 1.25;
	}
	.tile.warn .t-state {
		color: var(--warn);
	}
	.tile.ok .t-state {
		color: var(--ok);
	}
	.t-detail {
		margin: 0;
		font-size: var(--fs-md);
		color: var(--text-dim);
		line-height: 1.45;
	}
	.t-explain {
		margin: 3px 0 0;
		font-size: var(--fs-sm);
		color: var(--text-faint);
		line-height: 1.5;
	}
	.page {
		display: flex;
		flex-direction: column;
		gap: 10px;
		height: 100%;
		min-height: 0;
	}
	.head {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.head h1 {
		font-size: 1.2rem;
		font-weight: 600;
		margin: 0;
	}
	/* Verze systému vpravo — kontext, ne titulek. */
	.os-line {
		margin-left: auto;
		opacity: 0.7;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	/* Živé použití kamery/mikrofonu patří do hlavičky — je to ta
	   nejdůležitější informace celé sekce. */
	.live-warn {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		margin-left: auto;
		font-size: var(--fs-md);
		color: var(--danger);
		background: color-mix(in srgb, var(--danger) 12%, transparent);
		border: 1px solid color-mix(in srgb, var(--danger) 45%, transparent);
		border-radius: 999px;
		padding: 6px 13px;
	}
	.live-dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--danger);
		box-shadow: var(--glow-danger);
		animation: pulse 1.6s ease-in-out infinite;
	}
	@keyframes pulse {
		50% {
			opacity: 0.45;
		}
	}

	.body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding-right: 6px;
	}

	.sect {
		display: flex;
		align-items: center;
		gap: 9px;
		margin: 20px 0 9px;
		font-family: var(--font-mono);
		font-size: var(--fs-md);
		font-weight: 500;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--text-dim);
	}
	.sect:first-child {
		margin-top: 0;
	}
	.sect::after {
		content: '';
		flex: 1;
		height: 1px;
		background: var(--border);
	}
	.sect-n {
		font-weight: 400;
		font-size: var(--fs-xs);
		color: var(--text-faint);
		font-variant-numeric: tabular-nums;
	}
	.sect-live {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		font-size: var(--fs-xs);
		color: var(--danger);
		text-transform: none;
		letter-spacing: 0;
	}

	.item {
		display: grid;
		grid-template-columns: 40px minmax(0, 1fr) minmax(150px, auto);
		gap: 14px;
		align-items: center;
		padding: 12px 16px;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		margin-bottom: 8px;
		background: var(--surface);
	}
	.item.slim {
		grid-template-columns: minmax(0, 1fr) minmax(150px, auto);
		padding: 10px 16px;
	}
	.item:hover {
		background: var(--surface-hover);
	}
	.item.live {
		border-color: color-mix(in srgb, var(--danger) 45%, var(--border));
	}
	.ico {
		display: grid;
		place-items: center;
		width: 40px;
		height: 40px;
		border-radius: 11px;
		background: var(--surface-hover);
		color: var(--text-dim);
	}
	.info {
		min-width: 0;
	}
	.info h3 {
		margin: 0;
		font-size: 1rem;
		font-weight: 600;
		line-height: 1.3;
		word-break: break-word;
	}
	.perm-name {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.vendor {
		margin: 3px 0 0;
		font-size: var(--fs-sm);
		color: var(--text-dim);
		word-break: break-all;
	}
	.vers {
		display: inline-flex;
		align-items: center;
		gap: 3px;
		margin-left: 8px;
		padding: 1px 6px;
		border: 1px solid var(--border);
		border-radius: 999px;
		background: transparent;
		color: var(--text-faint);
		font-family: var(--font-mono);
		font-size: var(--fs-2xs);
		letter-spacing: 0.02em;
		cursor: pointer;
		vertical-align: middle;
	}
	.vers:hover {
		color: var(--text);
		border-color: var(--text-dim);
	}
	.vers.mixed {
		color: var(--warn);
		border-color: var(--warn);
	}
	/* Řádek staré verze: odsazený, tlumený — je to jen doklad. */
	.ver-row {
		margin-left: 22px;
		border-left: 2px solid var(--border);
		opacity: 0.72;
	}
	.mono {
		font-family: var(--font-mono);
		font-size: var(--fs-xs);
	}
	/* Stav a hodnoty jdou pod sebe, ne vedle sebe. Ve flexu v řádku
	   se dlouhá pilulka („odepřeno — nevynuceno") tlačila do jména
	   aplikace vlevo. */
	.side {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 7px;
		min-width: 0;
	}
	/* Stav oprávnění: řádek textu s barvou, ne velký barevný chip.
	   Chip u každého řádku dělal ze seznamu pruhovanou tabulku a
	   dlouhý popis se do něj nevešel. Mono verzálky ho drží ve
	   stejném jazyce jako popisky hodnot pod ním. */
	.p-state {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		font-family: var(--font-mono);
		font-size: var(--fs-2xs);
		letter-spacing: 0.04em;
		text-transform: uppercase;
		color: var(--text-faint);
		white-space: nowrap;
		text-align: right;
	}
	/* Zelená JEN u vynuceného blokování (balené aplikace). */
	.p-state.ok {
		color: var(--ok);
	}
	.p-state.warn {
		color: var(--warn);
	}
	.p-state.live {
		color: var(--danger);
	}


	.note {
		margin: 18px 0 12px;
		font-size: var(--fs-sm);
		line-height: 1.5;
		color: var(--text-dim);
	}
	.empty {
		color: var(--text-dim);
		font-size: var(--fs-lg);
		padding: 14px 0;
	}
</style>
