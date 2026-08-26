<script>
	// Po spuštění (v6, SPEC kap. 7): co startuje s Windows. Přepínač je
	// první REÁLNÁ mutace — jde přes validační vrstvu (třída T0: rychlá,
	// plně vratná, bez potvrzování). Nic se nemaže; Windows má na to
	// oficiální mechanismus, stejný jako Správce úloh.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
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
	import { isSystemApp, isSystemPath } from '$lib/mandatory.js';
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
		folder_user: { label: 'Složka po spuštění', icon: FolderOpen },
		folder_common: { label: 'Složka (všichni)', icon: FolderOpen },
		task: { label: 'Naplánovaná úloha', icon: CalendarClock },
		service: { label: 'Služba', icon: Cog },
		msix: { label: 'Store aplikace', icon: Package },
		shell: { label: 'Winlogon', icon: TriangleAlert }
	};
	const srcOf = (s) => sources[s] ?? { label: s, icon: Cog };

	// Systémové položky jsou ve výchozím stavu pryč. Verdikt počítá
	// služba (validate::system_startup_reason) a přijde jako pole
	// `system` — UI si ho nedopočítává, aby se dvě pravidla nemohla
	// rozejít. Přepnout je nejde tak jako tak; dlouhý seznam zamčených
	// řádků by jen zakryl to, co uživatel ovlivnit může.
	let visibleItems = $derived(prefs.showSystemStartup ? items : items.filter((i) => !i.system));

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
	// nezměnilo (to bylo vidět jako záseky při scrollování).
	let lastSig = '';

	async function load() {
		try {
			const fresh = await invoke('query_startup');
			loadError = '';
			const sig = fresh.map((i) => `${i.id}:${i.enabled}`).join('|');
			if (sig !== lastSig) {
				lastSig = sig;
				items = fresh;
				queueIcons(new Set(fresh.map((i) => i.identity_key).filter(Boolean)));
			}
		} catch (e) {
			loadError = String(e);
		}
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
				lastSig = items.map((i) => `${i.id}:${i.enabled}`).join("|");
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
						{#if isSystemApp({ identity_key: g.identity_key ?? '', display_name: g.label, publisher: g.publisher ?? '' }) || g.items.every((i) => !i.toggleable || isSystemPath(i.command))}
							<SystemBadge compact />
						{/if}
						<span class="g-count label-tech">{g.items.length}</span>
					</header>
					<ul class="items">
						{#each visible as i (i.id)}
							{@const s = srcOf(i.source)}
							<li class="item" class:off={!i.enabled}>
								<span class="i-src" title={s.label}><s.icon size={16} /></span>
								<span class="i-main">
									<span class="i-name">{i.name}</span>
									<span class="i-cmd mono" title={i.command}>{i.command}</span>
								</span>
								<span class="i-srclabel label-tech">{s.label}</span>
								<!-- U služby přepínač mění typ spuštění, ne aktuální
								     stav. Automatická služba může být zastavená, ručně
								     spouštěná může běžet — bez tohohle štítku se to
								     z řádku nedalo poznat a četlo se to obráceně. -->
								{#if i.running != null}
									<span
										class="i-run label-tech"
										class:live={i.running}
										title={i.running
											? 'Služba právě běží'
											: 'Služba je zastavená'}
									>
										{i.running ? 'běží' : 'stojí'}
									</span>
								{/if}
								{#if i.toggleable}
									<button
										class="sw"
										class:on={i.enabled}
										class:busy={busy.has(i.id)}
										onclick={() => toggle(i)}
										title={i.enabled ? 'Vypnout po spuštění' : 'Zapnout po spuštění'}
										aria-label="přepnout"
									>
										<span class="knob"></span>
									</button>
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
								{:else}
									<span class="locked" title="Systémová položka — jen k náhledu">
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
	.i-run {
		font-size: var(--fs-2xs);
		white-space: nowrap;
		min-width: 34px;
		text-align: right;
		opacity: 0.55;
	}
	.i-run.live {
		color: var(--ok);
		opacity: 1;
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
