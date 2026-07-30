<script>
	// Historie zásahů do systému (audit, SPEC 17.6) — vlastní stránka.
	// Každá akce, kterou Winsent provedl NEBO zamítl, nechává stopu:
	// bezpečnostní model stojí na tom, že to uživatel vidí.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { History, ShieldCheck, ShieldX, RotateCcw, RefreshCw } from 'lucide-svelte';

	let audit = $state([]);
	let loadError = $state('');
	let filter = $state('all'); // all | done | denied

	const actionLabel = {
		startup_toggle: 'startup přepínač',
		kill: 'ukončení procesu',
		test_toggle: 'testovací přepínač',
		test_op: 'testovací operace',
		check_proc: 'kontrola procesu'
	};

	function fmtTs(ts) {
		const d = new Date(ts * 1000);
		return d.toLocaleDateString('cs-CZ') + ' ' + d.toLocaleTimeString('cs-CZ');
	}

	let shown = $derived.by(() => {
		if (filter === 'done') return audit.filter((a) => a.verdict === 'allow');
		if (filter === 'denied') return audit.filter((a) => a.verdict === 'deny');
		return audit;
	});
	let counts = $derived.by(() => ({
		done: audit.filter((a) => a.verdict === 'allow').length,
		denied: audit.filter((a) => a.verdict === 'deny').length
	}));

	async function load() {
		try {
			audit = await invoke('query_audit', { limit: 300 });
			loadError = '';
		} catch (e) {
			loadError = String(e);
		}
	}

	onMount(() => {
		load();
		const t = setInterval(load, 10000);
		return () => clearInterval(t);
	});
</script>

<div class="page">
	<header class="head">
		<h1>Historie</h1>
		<span class="sub">co Winsent v systému udělal — a co zamítl</span>
		<div class="seg">
			<button class:active={filter === 'all'} onclick={() => (filter = 'all')}>
				Vše <i>{audit.length}</i>
			</button>
			<button class:active={filter === 'done'} onclick={() => (filter = 'done')}>
				Provedeno <i>{counts.done}</i>
			</button>
			<button class:active={filter === 'denied'} onclick={() => (filter = 'denied')}>
				Zamítnuto <i>{counts.denied}</i>
			</button>
		</div>
		<button class="refresh" onclick={load} title="Obnovit"><RefreshCw size={15} /></button>
	</header>

	{#if loadError}
		<p class="empty">{loadError}</p>
	{:else if audit.length === 0}
		<div class="empty explain">
			<p><b>Zatím žádné zásahy.</b></p>
			<p>
				Winsent do systému nic nemění sám od sebe. Jakmile něco přepneš nebo ukončíš,
				objeví se to tady — včetně akcí, které bezpečnostní vrstva zamítla a proč.
				Záznamy se nemažou.
			</p>
		</div>
	{:else}
		<ul class="list">
			{#each shown as a (a.id)}
				<li class="row" class:deny={a.verdict === 'deny'}>
					<span class="r-icon">
						{#if a.verdict === 'deny'}
							<ShieldX size={16} />
						{:else}
							<ShieldCheck size={16} />
						{/if}
					</span>
					<span class="r-main">
						<span class="r-top">
							<span class="r-act">{actionLabel[a.action] ?? a.action}</span>
							<span class="r-class label-tech">{a.class}</span>
							{#if a.verdict === 'deny'}
								<span class="r-badge deny">zamítnuto</span>
							{:else if a.outcome === 'ok'}
								<span class="r-badge ok">provedeno</span>
							{:else if a.outcome === 'rolled_back'}
								<span class="r-badge warn">vráceno zpět</span>
							{:else}
								<span class="r-badge warn">{a.outcome ?? '—'}</span>
							{/if}
						</span>
						<span class="r-target mono">{a.target}</span>
						{#if a.deny_reason}
							<span class="r-reason">{a.deny_reason}</span>
						{/if}
						{#if a.reversible}
							<span class="r-rev"><RotateCcw size={13} /> {a.reversible}</span>
						{/if}
					</span>
					<span class="r-ts mono">{fmtTs(a.ts)}</span>
				</li>
			{/each}
		</ul>
		<p class="note">
			<History size={13} /> Záznamy jsou trvalé — retence je nemaže. Sloupec vratnosti
			ukazuje, jak se akce dá vrátit.
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
		flex-wrap: wrap;
	}
	.head h1 {
		font-size: 1.2rem;
		font-weight: 600;
	}
	.sub {
		color: var(--text-faint);
		font-size: 0.84rem;
	}
	.seg {
		display: flex;
		gap: 2px;
		margin-left: auto;
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
		font-size: 0.64rem;
		color: var(--text-faint);
	}
	.seg button.active {
		background: var(--surface-hover);
		color: var(--text);
		box-shadow: inset 0 0 0 1px var(--border-strong);
	}
	.refresh {
		background: none;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text-dim);
		padding: 5px 7px;
		cursor: pointer;
		display: grid;
		place-items: center;
	}
	.refresh:hover {
		color: var(--text);
		border-color: var(--border-strong);
	}

	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		overflow-y: auto;
		min-height: 0;
		flex: 1;
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		background: var(--surface);
	}
	.row {
		display: flex;
		align-items: flex-start;
		gap: 12px;
		padding: 10px 14px;
		border-bottom: 1px dashed var(--border);
	}
	.r-icon {
		color: var(--ok);
		display: grid;
		place-items: center;
		padding-top: 2px;
	}
	.row.deny .r-icon {
		color: var(--danger);
	}
	.r-main {
		display: flex;
		flex-direction: column;
		gap: 3px;
		min-width: 0;
		flex: 1;
	}
	.r-top {
		display: flex;
		align-items: center;
		gap: 8px;
		flex-wrap: wrap;
	}
	.r-act {
		font-size: 0.9rem;
	}
	.r-class {
		font-size: 0.62rem;
	}
	.r-badge {
		font-size: 0.66rem;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		border-radius: 999px;
		padding: 1px 8px;
		border: 1px solid var(--border-strong);
		color: var(--text-dim);
	}
	.r-badge.ok {
		color: var(--ok);
		border-color: color-mix(in srgb, var(--ok) 40%, transparent);
	}
	.r-badge.deny {
		color: var(--danger);
		border-color: color-mix(in srgb, var(--danger) 40%, transparent);
	}
	.r-badge.warn {
		color: var(--warn);
		border-color: color-mix(in srgb, var(--warn) 40%, transparent);
	}
	.r-target {
		font-size: 0.78rem;
		color: var(--text-dim);
		word-break: break-all;
	}
	.r-reason {
		font-size: 0.78rem;
		color: var(--danger);
	}
	.r-rev {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		font-size: 0.74rem;
		color: var(--text-faint);
	}
	.r-ts {
		font-size: 0.72rem;
		color: var(--text-faint);
		white-space: nowrap;
	}
	.note {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 0.76rem;
		color: var(--text-faint);
	}
	.empty {
		color: var(--text-faint);
		font-size: 0.88rem;
		padding: 18px;
	}
	.empty.explain {
		max-width: 640px;
		line-height: 1.55;
	}
	.empty.explain p {
		margin: 0 0 10px;
	}
	.mono {
		font-family: var(--font-mono);
	}
</style>
