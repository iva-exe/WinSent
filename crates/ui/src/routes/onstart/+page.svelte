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
		History
	} from 'lucide-svelte';

	let items = $state([]);
	let filter = $state('');
	let segment = $state('all'); // all | on | off
	let loadError = $state('');
	let busy = $state(new Set());
	let toast = $state(null);
	let audit = $state([]);
	let showAudit = $state(false);

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
	async function fetchIcon(key) {
		if (!key || iconState.get(key) === 'done' || (iconState.get(key) ?? 0) >= 4) return;
		iconState.set(key, (iconState.get(key) ?? 0) + 1);
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

	let shown = $derived.by(() => {
		const f = filter.trim().toLowerCase();
		return items.filter((i) => {
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
			if (!map.has(key)) map.set(key, { label: key, identity_key: i.identity_key, items: [] });
			map.get(key).items.push(i);
		}
		return [...map.values()].sort((a, b) => b.items.length - a.items.length);
	});

	let counts = $derived.by(() => ({
		on: items.filter((i) => i.enabled).length,
		off: items.filter((i) => !i.enabled).length
	}));

	async function load() {
		try {
			items = await invoke('query_startup');
			loadError = '';
			for (const i of items) fetchIcon(i.identity_key);
		} catch (e) {
			loadError = String(e);
		}
	}

	async function loadAudit() {
		try {
			audit = await invoke('query_audit', { limit: 30 });
		} catch {
			audit = [];
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
		if (showAudit) loadAudit();
		setTimeout(() => (toast = null), 4000);
	}

	function fmtTs(ts) {
		const d = new Date(ts * 1000);
		return d.toLocaleDateString('cs-CZ') + ' ' + d.toLocaleTimeString('cs-CZ');
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
				Vše <i>{items.length}</i>
			</button>
			<button class:active={segment === 'on'} onclick={() => (segment = 'on')}>
				Zapnuté <i>{counts.on}</i>
			</button>
			<button class:active={segment === 'off'} onclick={() => (segment = 'off')}>
				Vypnuté <i>{counts.off}</i>
			</button>
		</div>
		<div class="filter">
			<Search size={14} />
			<input placeholder="hledat položku…" bind:value={filter} />
		</div>
		<button
			class="audit-btn"
			class:active={showAudit}
			onclick={() => {
				showAudit = !showAudit;
				if (showAudit) loadAudit();
			}}
			title="Historie zásahů do systému"
		>
			<History size={15} />
		</button>
	</header>

	{#if toast}
		<div class="toast {toast.kind}">{toast.text}</div>
	{/if}

	{#if showAudit}
		<section class="card audit">
			<h3><ShieldCheck size={14} /> Co Winsent v systému udělal</h3>
			{#if audit.length === 0}
				<p class="dim">zatím žádné zásahy</p>
			{:else}
				<ul>
					{#each audit as a (a.id)}
						<li>
							<span class="a-ts mono">{fmtTs(a.ts)}</span>
							<span class="a-act">{a.action}</span>
							<span class="a-target mono">{a.target}</span>
							<span class="a-verdict" class:deny={a.verdict === 'deny'}>
								{a.verdict === 'deny' ? 'zamítnuto' : (a.outcome ?? 'ok')}
							</span>
							{#if a.deny_reason}<span class="a-reason">{a.deny_reason}</span>{/if}
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	{/if}

	{#if loadError}
		<p class="empty">{loadError}</p>
	{:else}
		<div class="groups">
			{#each groups as g (g.label)}
				<section class="card grp">
					<header class="g-head">
						{#if g.identity_key && iconUrls[g.identity_key]}
							<img class="app-icon" src={iconUrls[g.identity_key]} alt="" />
						{:else}
							<span class="app-icon ph"></span>
						{/if}
						<span class="g-name">{g.label}</span>
						<span class="g-count label-tech">{g.items.length}</span>
					</header>
					<ul class="items">
						{#each g.items as i (i.id)}
							{@const s = srcOf(i.source)}
							<li class="item" class:off={!i.enabled}>
								<span class="i-src" title={s.label}><s.icon size={15} /></span>
								<span class="i-main">
									<span class="i-name">{i.name}</span>
									<span class="i-cmd mono" title={i.command}>{i.command}</span>
								</span>
								<span class="i-srclabel label-tech">{s.label}</span>
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
								{:else}
									<span class="locked" title="Systémová položka — jen k náhledu">
										<TriangleAlert size={15} />
									</span>
								{/if}
							</li>
						{/each}
					</ul>
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
		font-size: 0.78rem;
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
		font-size: 0.66rem;
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
		font-size: 0.82rem;
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
		font-size: 0.86rem;
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
		font-size: 0.8rem;
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
		font-size: 0.8rem;
		padding: 4px 0;
		border-bottom: 1px dashed var(--border);
	}
	.a-ts {
		font-size: 0.72rem;
		color: var(--text-faint);
	}
	.a-target {
		font-size: 0.74rem;
		color: var(--text-dim);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.a-verdict {
		color: var(--ok);
		font-size: 0.76rem;
	}
	.a-verdict.deny {
		color: var(--danger);
	}
	.a-reason {
		grid-column: 3 / -1;
		font-size: 0.72rem;
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
		font-size: 0.68rem;
	}
	.items {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.item {
		display: grid;
		grid-template-columns: 22px 1fr auto 44px;
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
		font-size: 0.88rem;
	}
	.i-cmd {
		font-size: 0.7rem;
		color: var(--text-faint);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		direction: rtl;
		text-align: left;
	}
	.i-srclabel {
		font-size: 0.64rem;
		white-space: nowrap;
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
	.locked {
		color: var(--warn);
		display: grid;
		place-items: center;
	}
	.foot {
		font-size: 0.78rem;
		color: var(--text-faint);
	}
	.mono {
		font-family: var(--font-mono);
	}
	.dim {
		color: var(--text-faint);
		font-size: 0.82rem;
	}
	.empty {
		color: var(--text-faint);
		padding: 16px;
	}
</style>
