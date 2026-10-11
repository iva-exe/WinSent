<script>
	// Po spuštění (v6, SPEC kap. 7): co startuje s Windows. Přepínač je
	// první REÁLNÁ mutace — jde přes validační vrstvu (třída T0: rychlá,
	// plně vratná, bez potvrzování). Nic se nemaže; Windows má na to
	// oficiální mechanismus, stejný jako Správce úloh.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { openMenu, akceKopirovat, akceOtevritUmisteni, oddelovac } from '$lib/itemmenu.svelte.js';
	import {
		Search,
		KeyRound,
		FolderOpen,
		CalendarClock,
		Cog,
		Package,
		TriangleAlert,
		ShieldCheck,
		History,
		ChevronDown,
		ChevronUp
	} from 'lucide-svelte';
	import SystemBadge from '$lib/SystemBadge.svelte';
	import { prefs } from '$lib/prefs.svelte.js';
	import AppIcon from '$lib/AppIcon.svelte';

	/// Kolik položek se v kartě ukáže před rozkliknutím (karty tak mají
	/// jednotnou výšku a stránka se nesype dlouhými seznamy).
	const COLLAPSED = 5;
	let expanded = $state(new Set());
	function toggleGroup(label) {
		const s = new Set(expanded);
		if (s.has(label)) s.delete(label);
		else s.add(label);
		expanded = s;
	}

	let items = $state([]);
	let filter = $state('');
	let segment = $state('all'); // all | on | off
	let loadError = $state('');
	let busy = $state(new Set());
	let toast = $state(null);

	// Ikony aplikací (sdílená cache služby, klíč = identity_key).
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
	// Ikony se tahají POSTUPNĚ a jen pro unikátní klíče — 360 položek
	// × paralelní IPC při každém pollu dřív sekalo celou stránku.
	let iconQueue = [];
	let iconRunning = false;
	async function drainIcons() {
		if (iconRunning) return;
		iconRunning = true;
		while (iconQueue.length) {
			const key = iconQueue.shift();
			try {
				const icon = await invoke('query_icon', { identityKey: key });
				if (icon) {
					iconUrls[key] = rgbaToUrl(icon);
					iconState.set(key, 'done');
				}
			} catch {
				/* služba mimo */
			}
		}
		iconRunning = false;
	}
	function queueIcons(keys) {
		for (const key of keys) {
			if (!key || iconState.has(key)) continue;
			iconState.set(key, 'queued');
			iconQueue.push(key);
		}
		drainIcons();
	}

	const sources = {
		run_user: { label: 'Registr (uživatel)', icon: KeyRound },
		run_machine: { label: 'Registr (systém)', icon: KeyRound },
		run_machine32: { label: 'Registr (systém, 32bit)', icon: KeyRound },
		run_once_user: { label: 'Registr (uživatel, jednou)', icon: KeyRound },
		run_once_machine: { label: 'Registr (systém, jednou)', icon: KeyRound },
		folder_user: { label: 'Složka po spuštění', icon: FolderOpen },
		folder_common: { label: 'Složka (všichni)', icon: FolderOpen },
		task: { label: 'Naplánovaná úloha', icon: CalendarClock },
		service: { label: 'Služba', icon: Cog },
		msix: { label: 'Store aplikace', icon: Package },
		shell: { label: 'Winlogon', icon: TriangleAlert }
	};
	const srcOf = (s) => sources[s] ?? { label: s, icon: Cog };

	// Bezpečnostní software (antivirus) služba zamyká stejně jako položky
	// Windows — `system` + důvod (PLAN §5) — protože jeho vypnutí patří do
	// jeho vlastního nastavení. Windows to ale NENÍ a štítek „součást
	// Windows" by u něj lhal. Rozlišit se to dá jen podle důvodu, který
	// služba posílá; kdyby ho přeformulovala, spadne položka zpět mezi
	// systémové — zamčená zůstane tak jako tak.
	const jeBezpecnostni = (i) => (i.system_reason ?? '').startsWith('bezpečnostní software');

	// Štítek „součást Windows" u skupiny jen tehdy, když verdikt služby
	// říká „Windows" u VŠECH jejích položek. Dřív stačila cesta se
	// \system32\ v příkazu — a služby ovladačů NVIDIA či Realteku z úložiště
	// ovladačů dostaly štítek Windows, přestože každá z nich měla živý
	// přepínač.
	const skupinaWindows = (g) => g.items.every((i) => i.system && !jeBezpecnostni(i));

	// Winlogon (Shell/Userinit) se tu ukazuje JEN v nestandardním stavu
	// — tedy právě když systému nepatří. Je to poplach, ne systémová
	// položka.
	const SHELL_ALARM = 'nestandardní hodnota — může jít o škodlivý software';

	// Systémové položky jsou ve výchozím stavu pryč. Verdikt počítá
	// služba (validate::system_startup_reason) a přijde jako pole
	// `system` — UI si ho nedopočítává, aby se dvě pravidla nemohla
	// rozejít. Přepnout je nejde tak jako tak; dlouhý seznam zamčených
	// řádků by jen zakryl to, co uživatel ovlivnit může. Bezpečnostní
	// software zůstává vidět i bez volby: Windows to není (volba se jmenuje
	// „Startovací položky Windows"), dřív tu byl vidět a dlaždice na
	// přehledu ho počítá mezi „nepatří Windows" — čísla se musí shodovat.
	let visibleItems = $derived(
		prefs.showSystemStartup ? items : items.filter((i) => !i.system || jeBezpecnostni(i))
	);

	let shown = $derived.by(() => {
		const f = filter.trim().toLowerCase();
		return visibleItems.filter((i) => {
			if (segment === 'on' && !i.enabled) return false;
			if (segment === 'off' && i.enabled) return false;
			if (
				f &&
				!i.name.toLowerCase().includes(f) &&
				!(i.app_name ?? '').toLowerCase().includes(f) &&
				!i.command.toLowerCase().includes(f)
			)
				return false;
			return true;
		});
	});

	// Seskupení podle aplikace („tohle spouští Adobe"), zbytek dle zdroje.
	let groups = $derived.by(() => {
		const map = new Map();
		for (const i of shown) {
			const key = i.app_name ?? srcOf(i.source).label;
			if (!map.has(key))
				map.set(key, {
					label: key,
					identity_key: i.identity_key,
					publisher: i.publisher,
					items: []
				});
			map.get(key).items.push(i);
		}
		return [...map.values()].sort((a, b) => b.items.length - a.items.length);
	});

	let counts = $derived.by(() => ({
		on: visibleItems.filter((i) => i.enabled).length,
		off: visibleItems.filter((i) => !i.enabled).length
	}));

	// Podpis seznamu — poll nesmí překreslit 360 řádků, když se nic
	// nezměnilo (to bylo vidět jako záseky při scrollování). Dřív v něm
	// bylo jen id:enabled, takže štítek „běží/stojí" zůstal navždy
	// u stavu z prvního načtení a později doinstalovaná aplikace se
	// k položce nepřiřadila. Jedna funkce pro load i toggle, ať se oba
	// podpisy nemůžou rozejít.
	const podpis = (arr) =>
		arr
			.map(
				(i) =>
					`${i.id}:${i.enabled}:${i.running}:${i.toggleable}:${i.system}:${i.identity_key ?? ''}:${i.command}:${i.caution ?? ''}`
			)
			.join('|');
	let lastSig = '';

	// Stejné id dvakrát (hodnota stejného jména v Run i RunOnce nebo
	// i ve WOW6432Node\Run) shodilo keyed each v produkčním buildu
	// (each_key_duplicate) a s ním celou stránku. Zdroj id opravuje
	// služba; tohle je pojistka pro starší službu — nechá se první
	// výskyt, protože přepínač i busy stejně pracují podle id.
	function bezDuplicit(arr) {
		const seen = new Set();
		return arr.filter((i) => !seen.has(i.id) && seen.add(i.id));
	}

	async function load() {
		try {
			const fresh = bezDuplicit(await invoke('query_startup'));
			loadError = '';
			const sig = podpis(fresh);
			if (sig !== lastSig) {
				lastSig = sig;
				items = fresh;
				queueIcons(new Set(fresh.map((i) => i.identity_key).filter(Boolean)));
			}
		} catch (e) {
			loadError = String(e);
		}
	}

	// Kontextové menu startovací položky.
	//
	// Vyhledává se jméno položky; když je obecné („Update", „Launcher"),
	// vezme `openMenu` jméno aplikace nebo binárku z příkazu. Celý
	// příkaz se neposílá — bývá v něm cesta se jménem uživatele.
	//
	// StartupRow pole exe_path nemá (je jen ve služebním BootItem), takže
	// „Otevřít umístění" se dřív neukázalo nikdy a do hledání šel poslední
	// díl celého příkazu i s argumenty a uvozovkou (`x.exe" --tray`).
	// Cesta se proto vytáhne z příkazu stejně jako collector-boot
	// exe_from_command: v uvozovkách do další uvozovky, jinak po první
	// `.exe`. %VAR% se tu rozvinout nedá — taková cesta se do Průzkumníku
	// neposílá (neexistuje), jen se z ní vezme jméno pro hledání.
	function exeZPrikazu(cmd) {
		const c = (cmd ?? '').trim();
		let p;
		if (c.startsWith('"')) {
			const end = c.indexOf('"', 1);
			p = end < 0 ? c.slice(1) : c.slice(1, end);
		} else {
			const at = c.toLowerCase().indexOf('.exe');
			if (at < 0) return null;
			p = c.slice(0, at + 4);
		}
		return p.toLowerCase().endsWith('.exe') ? p : null;
	}
	function menuPolozka(e, i) {
		const cesta = i.exe_path ?? exeZPrikazu(i.command);
		const exe = (cesta ?? '').split(/[\\/]/).pop() ?? '';
		// Jen absolutní cesta bez %VAR% — relativní `rundll32.exe` nebo
		// `%ProgramFiles%\…` by open_path stejně nenašel.
		const umisteni = cesta && /^([a-z]:\\|\\\\)/i.test(cesta) && !cesta.includes('%') ? cesta : null;
		openMenu(e, {
			title: i.name,
			subtitle: i.app_name ?? srcOf(i.source).label,
			hledat: [i.name, i.app_name, exe],
			kontext: 'po spuštění Windows',
			items: [
				{
					label: i.enabled ? 'Vypnout po spuštění' : 'Zapnout po spuštění',
					icon: 'power',
					disabled: !i.toggleable,
					hint: i.toggleable
						? i.enabled && i.caution
							? 'pozor'
							: ''
						: i.source === 'shell'
							? 'jen k náhledu'
							: (i.system_reason ?? 'patří Windows'),
					run: () => prepnout(i)
				},
				{
					label: 'Nastavení po spuštění ve Windows',
					icon: 'shield',
					run: () => invoke('open_settings_page', { page: 'startupapps' })
				},
				umisteni ? akceOtevritUmisteni(umisteni) : null,
				oddelovac,
				akceKopirovat(i.name),
				akceKopirovat(i.command, 'Kopírovat příkaz')
			]
		});
	}

	// Vypnutí položky, na které něco visí (součást ovladače, služba Office,
	// aktualizace WebView2), se nejdřív potvrdí s varováním ze služby.
	// Ostatní přepnutí zůstávají bez ptaní — jsou vratná a dialog u každé
	// položky by z potvrzení udělal rutinu, kterou nikdo nečte.
	// Zapnutí se nepotvrzuje nikdy: vrací stav, se kterým to funguje.
	let potvrdit = $state(null); // položka čekající na potvrzení vypnutí
	function prepnout(item) {
		if (!item.toggleable || busy.has(item.id)) return;
		if (item.enabled && item.caution) {
			potvrdit = item;
			return;
		}
		toggle(item);
	}
	function potvrzeno() {
		const item = potvrdit;
		potvrdit = null;
		if (item) toggle(item);
	}

	// Přepnutí: optimisticky překlopit, po odpovědi srovnat s realitou.
	async function toggle(item) {
		if (!item.toggleable || busy.has(item.id)) return;
		busy = new Set(busy).add(item.id);
		const want = !item.enabled;
		try {
			const r = await invoke('toggle_startup', { id: item.id, on: want });
			if (r.verdict === 'allow' && r.outcome === 'ok') {
				items = items.map((i) => (i.id === item.id ? { ...i, enabled: want } : i));
				lastSig = podpis(items);
				toast = {
					kind: 'ok',
					text: `${item.name}: ${want ? 'zapnuto' : 'vypnuto'} (${r.duration_ms} ms)`
				};
			} else {
				toast = {
					kind: 'deny',
					text: r.deny_reason ?? `nepodařilo se (${r.outcome ?? 'chyba'})`
				};
				load();
			}
		} catch (e) {
			toast = { kind: 'deny', text: String(e) };
		}
		const b = new Set(busy);
		b.delete(item.id);
		busy = b;
		setTimeout(() => (toast = null), 4000);
	}


	onMount(() => {
		load();
		const t = setInterval(load, 30000);
		return () => clearInterval(t);
	});
</script>

<div class="page">
	<header class="head">
		<h1>Po spuštění</h1>
		<div class="seg">
			<button class:active={segment === 'all'} onclick={() => (segment = 'all')}>
				Vše <i>{visibleItems.length}</i>
			</button>
			<button class:active={segment === 'on'} onclick={() => (segment = 'on')}>
				Zapnuté <i>{counts.on}</i>
			</button>
			<button class:active={segment === 'off'} onclick={() => (segment = 'off')}>
				Vypnuté <i>{counts.off}</i>
			</button>
		</div>
		<div class="filter">
			<Search size={15} />
			<input placeholder="hledat položku…" bind:value={filter} />
		</div>
		<a class="audit-btn" href="/history" title="Historie zásahů do systému">
			<History size={16} />
		</a>
	</header>

	{#if toast}
		<div class="toast {toast.kind}">{toast.text}</div>
	{/if}

	<!-- Potvrzení vypnutí položky s varováním ze služby. -->
	{#if potvrdit}
		<div class="dlg-backdrop" role="presentation" onclick={() => (potvrdit = null)} onkeydown={() => {}}>
			<div class="dlg" role="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
				<h2>Vypnout {potvrdit.name} po spuštění?</h2>
				<p class="d-warn"><TriangleAlert size={15} /> {potvrdit.caution}</p>
				<p class="d-note">Vypnutí nic nemaže — položku tady kdykoli zase zapneš.</p>
				<div class="d-actions">
					<button class="d-btn" onclick={() => (potvrdit = null)}>Zrušit</button>
					<button class="d-btn primary" onclick={potvrzeno}>Vypnout</button>
				</div>
			</div>
		</div>
	{/if}

	{#if loadError}
		<p class="empty">{loadError}</p>
	{:else}
		<div class="groups">
			{#each groups as g (g.label)}
				{@const open = expanded.has(g.label)}
				{@const visible = open ? g.items : g.items.slice(0, COLLAPSED)}
				<section class="card grp" class:collapsed={!open && g.items.length > COLLAPSED}>
					<header class="g-head">
						<AppIcon src={g.identity_key ? iconUrls[g.identity_key] : null} name={g.label} size={21} />
						<span class="g-name">{g.label}</span>
						{#if skupinaWindows(g)}
							<SystemBadge compact title="Součást Windows — Winsent ji nepřepíná" />
						{/if}
						<span class="g-count label-tech">{g.items.length}</span>
					</header>
					<ul class="items">
						{#each visible as i (i.id)}
							{@const s = srcOf(i.source)}
							<li class="item" class:off={!i.enabled} oncontextmenu={(e) => menuPolozka(e, i)}>
								<span class="i-src" title={s.label}><s.icon size={16} /></span>
								<span class="i-main">
									<span class="i-title">
										<span class="i-name">{i.name}</span>
										<!-- U služby přepínač mění typ spuštění, ne aktuální
										     stav. Automatická služba může být zastavená, ručně
										     spouštěná může běžet — bez tohohle štítku se to
										     z řádku nedalo poznat a četlo se to obráceně.
										     Patří k názvu, ne k pravému okraji: tam mizel
										     mezi zdrojem a přepínačem. -->
										{#if i.running != null}
											<span
												class="i-run"
												class:live={i.running}
												title={i.running ? 'Služba právě běží' : 'Služba je zastavená'}
											>
												{i.running ? 'běží' : 'stojí'}
											</span>
										{/if}
									</span>
									<span class="i-cmd mono" title={i.command}>{i.command}</span>
									<!-- Varování ze služby: co přestane fungovat, když se
									     položka vypne. Rovnou v řádku, ne až v dialogu —
									     rozhoduje se podle toho, jestli na ni sahat. -->
									{#if i.caution}
										<span class="i-caution"><TriangleAlert size={12} /> {i.caution}</span>
									{:else if i.source === 'shell'}
										<span class="i-caution alarm"><TriangleAlert size={12} /> {SHELL_ALARM}</span>
									{/if}
								</span>
								<span class="i-srclabel label-tech">{s.label}</span>
								{#if i.toggleable}
									<button
										class="sw"
										class:on={i.enabled}
										class:busy={busy.has(i.id)}
										onclick={() => prepnout(i)}
										title={i.enabled ? 'Vypnout po spuštění' : 'Zapnout po spuštění'}
										aria-label="přepnout"
									>
										<span class="knob"></span>
									</button>
								{:else if i.system && jeBezpecnostni(i)}
									<!-- Antivirus: zamčený jako Windows, ale Windows to
									     není — jiný popisek i barva. -->
									<span class="locked secitem" title="{i.system_reason}. Winsent ho nepřepíná.">
										<ShieldCheck size={16} />
									</span>
								{:else if i.system}
									<!-- Položka Windows. Přepínač tu není schválně:
									     službě by ho validační vrstva stejně odmítla
									     a nabízet nefunkční ovladač je horší než ho
									     nenabízet. Důvod chodí ze služby. -->
									<span
										class="locked sysitem"
										title="Patří Windows{i.system_reason ? ` — ${i.system_reason}` : ''}. Winsent startovací položky systému nepřepíná."
									>
										<ShieldCheck size={16} />
									</span>
								{:else if i.source === 'shell'}
									<span class="locked alarm" title="Winlogon: {SHELL_ALARM}. Jen k náhledu — Winsent ho nepřepíná.">
										<TriangleAlert size={16} />
									</span>
								{:else}
									<span class="locked" title={i.system_reason ?? 'Systémová položka — jen k náhledu'}>
										<TriangleAlert size={16} />
									</span>
								{/if}
							</li>
						{/each}
					</ul>
					{#if g.items.length > COLLAPSED}
						<button class="more" onclick={() => toggleGroup(g.label)}>
							{#if open}
								<ChevronUp size={15} /> sbalit
							{:else}
								<ChevronDown size={15} /> zobrazit všech {g.items.length}
							{/if}
						</button>
					{/if}
				</section>
			{/each}
		</div>
		<p class="foot">
			Přepnutí nic nemaže — Windows si vypnutou položku jen poznamená (stejně jako Správce
			úloh), takže jde kdykoli vrátit. Každá změna projde bezpečnostní vrstvou a zapíše se
			do historie zásahů.
		</p>
	{/if}
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: 14px;
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
	}
	.seg {
		display: flex;
		gap: 2px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 2px;
		background: var(--surface);
	}
	.seg button {
		background: none;
		border: none;
		color: var(--text-dim);
		font: inherit;
		font-size: var(--fs-sm);
		padding: 4px 10px;
		border-radius: 3px;
		cursor: pointer;
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.seg button i {
		font-style: normal;
		font-family: var(--font-mono);
		font-size: var(--fs-2xs);
		color: var(--text-faint);
	}
	.seg button.active {
		background: var(--surface-hover);
		color: var(--text);
		box-shadow: inset 0 0 0 1px var(--border-strong);
	}
	.filter {
		margin-left: auto;
		display: flex;
		align-items: center;
		gap: 6px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 5px 9px;
		color: var(--text-dim);
		background: var(--surface);
	}
	.filter input {
		background: none;
		border: none;
		outline: none;
		color: var(--text);
		font: inherit;
		font-size: var(--fs-md);
		width: 170px;
	}
	.audit-btn {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text-dim);
		padding: 6px 8px;
		cursor: pointer;
		display: grid;
		place-items: center;
	}
	.audit-btn.active,
	.audit-btn:hover {
		color: var(--text);
		border-color: var(--border-strong);
	}

	.toast {
		padding: 9px 14px;
		border-radius: var(--radius-sm);
		font-size: var(--fs-lg);
		border: 1px solid var(--border-strong);
		background: var(--surface);
	}
	.toast.ok {
		border-color: color-mix(in srgb, var(--ok) 45%, transparent);
		color: var(--ok);
	}
	.toast.deny {
		border-color: color-mix(in srgb, var(--danger) 45%, transparent);
		color: var(--danger);
	}

	.card {
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 12px 14px;
	}
	.audit h3 {
		display: flex;
		align-items: center;
		gap: 7px;
		font-size: var(--fs-md);
		text-transform: uppercase;
		letter-spacing: 0.06em;
		color: var(--text-dim);
		margin: 0 0 8px;
		font-weight: 500;
	}
	.audit ul {
		list-style: none;
		margin: 0;
		padding: 0;
		max-height: 26vh;
		overflow-y: auto;
	}
	.audit li {
		display: grid;
		grid-template-columns: 150px 110px 1fr auto;
		gap: 10px;
		align-items: baseline;
		font-size: var(--fs-md);
		padding: 4px 0;
		border-bottom: 1px dashed var(--border);
	}
	.a-ts {
		font-size: var(--fs-xs);
		color: var(--text-faint);
	}
	.a-target {
		font-size: var(--fs-xs);
		color: var(--text-dim);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.a-verdict {
		color: var(--ok);
		font-size: var(--fs-sm);
	}
	.a-verdict.deny {
		color: var(--danger);
	}
	.a-reason {
		grid-column: 3 / -1;
		font-size: var(--fs-xs);
		color: var(--text-faint);
	}

	.groups {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(460px, 1fr));
		gap: 12px;
		overflow-y: auto;
		align-content: start;
		min-height: 0;
	}
	.g-head {
		display: flex;
		align-items: center;
		gap: 9px;
		margin-bottom: 8px;
	}
	.app-icon {
		width: 20px;
		height: 20px;
		border-radius: 3px;
		flex: none;
	}
	.app-icon.ph {
		background: var(--surface-hover);
		border: 1px dashed var(--border);
		display: inline-block;
	}
	.g-name {
		font-size: 0.95rem;
		font-weight: 500;
		flex: 1;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.g-count {
		font-size: var(--fs-2xs);
	}
	/* Karty mají jednotnou výšku; delší seznam se sbalí s fadem.
	   `content-visibility` platí JEN pro sbalené karty — u rozbalené
	   by zástupná výška rozbila výpočet scrollu (nešlo doscrollovat
	   na konec). Sbalená karta má vždy stejnou výšku, takže je
	   zástupná hodnota přesná. */
	.grp {
		display: flex;
		flex-direction: column;
		position: relative;
	}
	.grp.collapsed {
		content-visibility: auto;
		contain-intrinsic-size: auto 268px;
	}
	.items {
		list-style: none;
		margin: 0;
		padding: 0;
		flex: 1;
	}
	/* Fade patří na SPODNÍ HRANU KARTY (ne nad tlačítko) — kreslí se
	   proto na kartě, přes celou její šířku, a tlačítko „zobrazit vše"
	   leží nad ním. Pseudoelement místo mask-image: maska nutila
	   prohlížeč skládat každou kartu zvlášť a scroll se sekal. */
	.grp.collapsed::after {
		content: '';
		position: absolute;
		left: 0;
		right: 0;
		bottom: 0;
		height: 76px;
		pointer-events: none;
		background: linear-gradient(to bottom, rgba(22, 23, 28, 0), rgba(22, 23, 28, 0.96) 68%);
		border-radius: 0 0 var(--radius) var(--radius);
	}
	.grp .more {
		position: relative;
		z-index: 1;
	}
	.more {
		margin-top: 6px;
		align-self: flex-start;
		display: flex;
		align-items: center;
		gap: 6px;
		background: none;
		border: none;
		color: var(--text-dim);
		font: inherit;
		font-size: var(--fs-sm);
		cursor: pointer;
		padding: 2px 0;
	}
	.more:hover {
		color: var(--text);
	}
	.item {
		display: grid;
		grid-template-columns: 22px minmax(0, 1fr) auto 44px;
		gap: 10px;
		align-items: center;
		padding: 7px 0;
		border-top: 1px dashed var(--border);
	}
	.item.off .i-name,
	.item.off .i-cmd {
		color: var(--text-faint);
	}
	.i-src {
		color: var(--text-dim);
		display: grid;
		place-items: center;
	}
	.i-main {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.i-name {
		font-size: var(--fs-xl);
		/* Dlouhé názvy (GUID balíčky, cesty) se zalomí, ať nelezou
		   do přepínače. */
		overflow-wrap: anywhere;
		display: flex;
		align-items: center;
		gap: 5px;
		flex-wrap: wrap;
	}
	.i-cmd {
		font-size: var(--fs-xs);
		color: var(--text-faint);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		direction: rtl;
		text-align: left;
	}
	.i-srclabel {
		font-size: var(--fs-2xs);
		white-space: nowrap;
	}
	/* Název a stav běhu na jednom řádku; stav je malý odznak vedle
	   jména, ne sloupec u pravého okraje. */
	.i-title {
		display: flex;
		align-items: baseline;
		gap: 7px;
		min-width: 0;
		flex-wrap: wrap;
	}
	.i-run {
		font-size: var(--fs-xs);
		white-space: nowrap;
		color: var(--text-faint);
	}
	.i-run.live {
		color: var(--ok);
	}
	/* Přepínač — vratná akce, žádný dialog (T0). */
	.sw {
		width: 38px;
		height: 21px;
		border-radius: 999px;
		border: 1px solid var(--border-strong);
		background: var(--panel);
		position: relative;
		cursor: pointer;
		padding: 0;
		transition:
			background 0.18s ease,
			border-color 0.18s ease;
	}
	.sw .knob {
		position: absolute;
		top: 2px;
		left: 2px;
		width: 15px;
		height: 15px;
		border-radius: 50%;
		background: var(--text-faint);
		transition:
			transform 0.18s ease,
			background 0.18s ease;
	}
	.sw.on {
		background: color-mix(in srgb, var(--ok) 26%, transparent);
		border-color: color-mix(in srgb, var(--ok) 55%, transparent);
	}
	.sw.on .knob {
		transform: translateX(17px);
		background: var(--ok);
		box-shadow: var(--glow-ok);
	}
	.sw.busy {
		opacity: 0.5;
		cursor: wait;
	}
	/* Zámek u položky Windows — modrý štít, ne jantarový vykřičník:
	   není to problém, je to prostě systém. */
	.locked.sysitem {
		color: var(--net-down);
	}
	/* Antivirus — štít, ale ne modrý: modrá v aplikaci znamená Windows. */
	.locked.secitem {
		color: var(--text-dim);
	}
	/* Nestandardní Winlogon je poplach, ne „jen k náhledu". */
	.locked.alarm {
		color: var(--danger);
	}
	/* Co přestane fungovat, když se položka vypne (ze služby). */
	.i-caution {
		display: flex;
		align-items: center;
		gap: 5px;
		font-size: var(--fs-xs);
		color: var(--warn);
		margin-top: 2px;
	}
	.i-caution :global(svg) {
		flex: none;
	}
	.i-caution.alarm {
		color: var(--danger);
	}

	/* Potvrzení vypnutí — stejný jazyk jako dialogy v Programs a Tasks. */
	.dlg-backdrop {
		position: fixed;
		inset: 0;
		background: rgba(8, 9, 11, 0.62);
		display: grid;
		place-items: center;
		z-index: 40;
	}
	.dlg {
		width: min(520px, 92vw);
		background: #16171c;
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-lg);
		padding: 20px 22px;
	}
	.dlg h2 {
		font-size: 1.05rem;
		margin: 0 0 10px;
		overflow-wrap: anywhere;
	}
	.d-warn {
		display: flex;
		gap: 7px;
		align-items: flex-start;
		font-size: var(--fs-lg);
		color: var(--warn);
		margin: 0 0 10px;
		line-height: 1.45;
	}
	.d-warn :global(svg) {
		flex: none;
		margin-top: 3px;
	}
	.d-note {
		font-size: var(--fs-md);
		color: var(--text-faint);
		margin: 0 0 14px;
	}
	.d-actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
	}
	.d-btn {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text);
		font: inherit;
		font-size: var(--fs-lg);
		padding: 7px 14px;
		cursor: pointer;
	}
	.d-btn:hover {
		border-color: var(--border-strong);
	}
	.d-btn.primary {
		border-color: color-mix(in srgb, var(--warn) 50%, transparent);
		color: var(--warn);
	}
	.locked {
		color: var(--warn);
		display: grid;
		place-items: center;
	}
	.foot {
		font-size: var(--fs-sm);
		color: var(--text-faint);
	}
	.mono {
		font-family: var(--font-mono);
	}
	.dim {
		color: var(--text-faint);
		font-size: var(--fs-md);
	}
	.empty {
		color: var(--text-faint);
		padding: 16px;
	}
</style>
