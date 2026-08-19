<script>
	// Users (v9E, SPEC kap. 14): kdo se na tenhle počítač dostane a kdo
	// tu může všechno. Sekce jen čte — zakládat účty a měnit práva umí
	// Windows samy a dělají to líp.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import {
		ShieldCheck,
		User,
		UserCog,
		UserX,
		Lock,
		Mail,
		Globe,
		KeyRound
	} from 'lucide-svelte';

	let report = $state(null);
	let loadError = $state(null);

	async function load() {
		try {
			report = await invoke('query_users');
			loadError = null;
		} catch (e) {
			loadError = String(e);
		}
	}

	onMount(() => {
		load();
		// Účty se mění zřídka; služba je navíc drží minutu v cache.
		const t = setInterval(load, 60000);
		return () => clearInterval(t);
	});

	// Správci nahoru — to je otázka, kvůli které sem člověk jde.
	// Vypnuté účty dolů: existují, ale nikoho k ničemu nepustí.
	let users = $derived(
		[...(report?.users ?? [])].sort(
			(a, b) =>
				a.disabled - b.disabled ||
				b.admin - a.admin ||
				a.name.localeCompare(b.name, 'cs')
		)
	);

	let admins = $derived(
		users.filter((u) => u.admin && !u.disabled).length + (report?.foreign_admins?.length ?? 0)
	);

	function fmtWhen(ts) {
		if (!ts) return 'nikdy';
		const d = new Date(ts * 1000);
		const days = Math.floor((Date.now() - d.getTime()) / 86400e3);
		const t = d.toLocaleDateString('cs-CZ');
		if (days <= 0) return `dnes v ${d.toLocaleTimeString('cs-CZ', { hour: '2-digit', minute: '2-digit' })}`;
		if (days === 1) return 'včera';
		if (days < 30) return `před ${days} dny`;
		return t;
	}

	// Ikona podle toho, čím účet JE — ne podle toho, jak se jmenuje.
	function icoOf(u) {
		if (u.disabled) return UserX;
		if (u.admin) return UserCog;
		return User;
	}
</script>

<div class="page">
	<header class="head">
		<h1>Users</h1>
		<span class="label-tech">
			{users.length}
			{users.length === 1 ? 'účet' : 'účtů'} · {admins} se správcovskými právy
		</span>
		{#if report?.current_user}
			<span class="me label-tech">přihlášen: {report.current_user}</span>
		{/if}
	</header>

	{#if loadError}
		<p class="empty">Nelze načíst účty: {loadError}</p>
	{:else if report}
		<div class="body">
			<h2 class="sect">
				<ShieldCheck size={16} /> Účty na tomhle počítači
				<span class="sect-n">{users.length}</span>
			</h2>

			{#each users as u (u.sid || u.name)}
				{@const Ico = icoOf(u)}
				{@const me = u.name.toLowerCase() === (report.current_user ?? '').toLowerCase()}
				<article class="item" class:off={u.disabled}>
					<div class="ico"><Ico size={19} /></div>
					<div class="info">
						<h3>
							{u.name}
							{#if me}<span class="tag me-tag">to jsi ty</span>{/if}
							{#if u.full_name && u.full_name !== u.name}
								<span class="full">{u.full_name}</span>
							{/if}
						</h3>
						{#if u.comment}<p class="vendor">{u.comment}</p>{/if}
						<div class="facts">
							<!-- Poslední přihlášení Windows nesdílejí mezi počítači,
							     takže se to musí říct — jinak by číslo lhalo o tom,
							     kde všude se účet používá. -->
							<span class="fact" title="Windows tenhle údaj vedou jen pro tenhle počítač">
								naposledy zde {fmtWhen(u.last_logon)}
							</span>
							{#if u.logons}
								<span class="fact muted">{u.logons}× přihlášení</span>
							{/if}
							{#if u.microsoft}
								<span class="fact muted"><Mail size={12} /> účet Microsoft</span>
							{/if}
							{#if u.password_not_required && !u.disabled}
								<!-- Fakt, ne poplach. Příznak říká „Windows tu heslo
								     nevyžadují", ne „účet žádné nemá" — u účtů
								     Microsoft bývá nastavený úplně běžně. -->
								<span
									class="fact muted"
									title="Windows u tohohle účtu heslo nevyžadují. Neznamená to, že žádné nemá — u účtů propojených s účtem Microsoft bývá tenhle příznak nastavený běžně."
								>
									heslo není vyžadováno
								</span>
							{/if}
							<span class="fact mono muted">{u.sid}</span>
						</div>
					</div>
					<div class="side">
						{#if u.disabled}
							<span class="pill quiet">vypnutý</span>
						{:else if u.admin}
							<!-- Admin práva nejsou chyba, ale je to ta věc, kvůli
							     které se sem člověk dívá. Proto výrazně. -->
							<span class="pill admin"><KeyRound size={13} /> správce</span>
						{:else}
							<span class="pill quiet">běžný účet</span>
						{/if}
						{#if u.locked}
							<span class="pill warn"><Lock size={13} /> zamčený</span>
						{/if}
					</div>
				</article>
			{/each}

			{#if report.foreign_admins?.length}
				<h2 class="sect">
					<Globe size={16} /> Správci mimo tenhle počítač
					<span class="sect-n">{report.foreign_admins.length}</span>
				</h2>
				<p class="note">
					Ve skupině „{report.admin_group}" jsou i tihle — firemní účty a skupiny, které
					v místní databázi účtů nejsou. Práva správce tu mají stejná.
				</p>
				{#each report.foreign_admins as f (f.sid)}
					<article class="item">
						<div class="ico"><Globe size={19} /></div>
						<div class="info">
							<h3>{f.name || f.sid}</h3>
							<div class="facts">
								<span class="fact muted">{f.kind}</span>
								<span class="fact mono muted">{f.sid}</span>
							</div>
						</div>
						<div class="side">
							<span class="pill admin"><KeyRound size={13} /> správce</span>
						</div>
					</article>
				{/each}
			{/if}

			<p class="note">
				Seznam jsou účty vedené na tomhle počítači — ne všichni, kdo se sem kdy přihlásili.
				Winsent účty jen čte: zakládat je a měnit práva umí Windows samy a mají na to
				nástroje, které tomu rozumí líp.
			</p>
		</div>
	{:else}
		<p class="empty">Načítám účty…</p>
	{/if}
</div>

<style>
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
	.me {
		margin-left: auto;
		color: var(--text-faint);
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
	.item {
		display: flex;
		align-items: flex-start;
		gap: 12px;
		padding: 11px 13px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		margin-bottom: 7px;
	}
	/* Vypnutý účet je fakt, ne varování — proto jen ustoupí. */
	.item.off {
		opacity: 0.6;
	}
	.ico {
		color: var(--text-dim);
		display: flex;
		padding-top: 2px;
	}
	.info {
		flex: 1;
		min-width: 0;
	}
	.info h3 {
		margin: 0;
		font-size: 1.02rem;
		font-weight: 600;
		line-height: 1.3;
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 8px;
	}
	.full {
		font-size: var(--fs-md);
		font-weight: 400;
		color: var(--text-dim);
	}
	.tag {
		font-family: var(--font-mono);
		font-size: var(--fs-3xs);
		letter-spacing: 0.04em;
		padding: 1px 6px;
		border-radius: 999px;
		border: 1px solid var(--border);
		color: var(--text-faint);
	}
	.me-tag {
		border-color: var(--ok);
		color: var(--ok);
	}
	.vendor {
		margin: 3px 0 0;
		font-size: var(--fs-md);
		color: var(--text-dim);
	}
	.facts {
		display: flex;
		flex-wrap: wrap;
		gap: 6px 8px;
		margin-top: 6px;
	}
	.fact {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		font-size: var(--fs-xs);
		color: var(--text-dim);
	}
	.fact.muted {
		color: var(--text-faint);
	}
	.fact.mono {
		font-family: var(--font-mono);
		font-size: var(--fs-2xs);
		word-break: break-all;
	}
	.side {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 5px;
		flex-shrink: 0;
	}
	.pill {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		padding: 3px 10px;
		border-radius: 999px;
		border: 1px solid var(--border);
		font-size: var(--fs-xs);
		white-space: nowrap;
	}
	.pill.quiet {
		color: var(--text-faint);
	}
	.pill.admin {
		color: var(--warn);
		border-color: color-mix(in srgb, var(--warn) 55%, transparent);
		background: color-mix(in srgb, var(--warn) 10%, transparent);
	}
	.pill.warn {
		color: var(--danger);
		border-color: color-mix(in srgb, var(--danger) 55%, transparent);
	}
	.note {
		margin: 14px 0 0;
		font-size: var(--fs-sm);
		line-height: 1.55;
		color: var(--text-faint);
	}
	.empty {
		color: var(--text-faint);
		font-size: var(--fs-lg);
	}
</style>
