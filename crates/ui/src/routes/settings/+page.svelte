<script>
	// Settings — v1 zatím jen dlaždice „Spotřeba nástroje“ (SPEC kap. 2.3):
	// rozpočet démona musí být pro uživatele ověřitelný, ne slibovaný.
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { daemon } from '$lib/daemon.svelte.js';
	import Num from '$lib/Num.svelte';
	import { prefs, setPref } from '$lib/prefs.svelte.js';

	import { updater, checkUpdate, runUpdate } from '$lib/updater.svelte.js';
	import { gatherAll, reportText } from '$lib/pcreport.js';
	import { Download, FolderOpen, RefreshCw, ShieldCheck } from 'lucide-svelte';
	// Kdy naposledy. Přesný čas nikoho nezajímá, „před chvílí" ano.

	// ── Záznam o celém počítači ──
	// Stejný účel i tvar jako záznam o incidentu: textový soubor, který
	// se dá poslat člověku nebo modelu. Sběr trvá — bere se stav ze všech
	// sekcí plus záznamy za posledních 24 hodin —, takže tlačítko musí
	// průběžně říkat, co zrovna dělá.
	let pcState = $state('idle'); // idle | busy | done | error
	let pcStep = $state('');
	let pcPath = $state('');

	async function downloadPcReport() {
		if (pcState === 'busy') return;
		pcState = 'busy';
		pcStep = 'začínám';
		try {
			const data = await gatherAll(invoke, (s) => (pcStep = s));
			const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-');
			pcStep = 'skládám soubor';
			const name = `winsent-pocitac-${stamp}.txt`;
			pcPath = await invoke('save_report', { name, text: reportText(data) });
			pcState = 'done';
			// Otevřít složku a soubor v ní označit — uživatel ho nemusí hledat.
			try {
				await invoke('open_path', { path: pcPath });
			} catch {
				/* Průzkumník se neotevřel; cesta zůstane vypsaná */
			}
			setTimeout(() => {
				if (pcState === 'done') pcState = 'idle';
			}, 12000);
		} catch (e) {
			pcStep = String(e);
			pcState = 'error';
			setTimeout(() => {
				if (pcState === 'error') pcState = 'idle';
			}, 12000);
		}
	}

	// Kdy naposledy se kontrolovalo. Kontrola běží každých 30 s, takže
	// 'před chvílí' by tam svítilo pořád — pod minutu se tiká po
	// sekundách, ať je vidět, že se opravdu něco děje.
	let ted = $state(Date.now());
	let lastCheck = $derived.by(() => {
		if (!updater.checkedAt) return 'zatím ne';
		const s = Math.max(0, Math.round((ted - updater.checkedAt) / 1000));
		if (s < 5) return 'právě teď';
		if (s < 60) return `před ${s} s`;
		const m = Math.round(s / 60);
		return m < 90 ? `před ${m} min` : `před ${Math.round(m / 60)} h`;
	});

	// Krátké bliknutí u čísel verzí, když kontrola doběhne. Bez něj
	// nešlo poznat, jestli tlačítko vůbec něco udělalo — většina
	// kontrol totiž skončí tím, že se nic nezměnilo.
	let justChecked = $state(false);
	let posledniBlik = 0;
	$effect(() => {
		const t = updater.checkedAt;
		if (!t || t === posledniBlik) return;
		posledniBlik = t;
		justChecked = true;
		const id = setTimeout(() => (justChecked = false), 1000);
		return () => clearTimeout(id);
	});

	// ── Kam se ukládá databáze ──
	//
	// Přesun se NEDĚJE odsud: databáze je otevřená a stěhovat ji pod
	// rukama by znamenalo přijít o rozepsaný WAL. Uloží se přání a
	// zbytek udělá start služby.
	let db = $state(null);
	let dbErr = $state('');
	let dbMsg = $state('');
	let dbBusy = $state(false);

	async function nacistDb() {
		try {
			db = await invoke('query_db_location');
			dbErr = '';
		} catch (e) {
			db = null;
			dbErr = String(e);
		}
	}

	async function vybratSlozku() {
		if (dbBusy) return;
		try {
			const dir = await invoke('pick_folder');
			if (dir) await ulozitSlozku(dir);
		} catch (e) {
			dbErr = String(e);
		}
	}

	async function ulozitSlozku(dir) {
		dbBusy = true;
		dbMsg = '';
		try {
			await invoke('set_db_dir', { dir });
			await nacistDb();
			dbMsg = dir ? 'Uloženo.' : 'Vrátí se na výchozí místo.';
		} catch (e) {
			dbErr = String(e);
		}
		dbBusy = false;
	}

	// Restart služby umí jen instalátor — UI běží pod běžným uživatelem
	// a na správu služeb nedosáhne. Stejnou cestou se služba zvedá i po
	// ručním zastavení, takže se nic nového nevymýšlí.
	async function restartSluzby() {
		dbBusy = true;
		try {
			await invoke('repair_service');
			dbMsg = 'Služba se restartuje — po naběhnutí se databáze přesune.';
		} catch (e) {
			dbErr = String(e);
		}
		dbBusy = false;
	}

	let usage = $state(null);
	let error = $state('');

	async function refresh() {
		try {
			usage = await invoke('query_self_usage');
			error = '';
		} catch (e) {
			usage = null;
			error = String(e);
		}
	}

	function fmtBytes(b) {
		const mb = b / (1024 * 1024);
		return mb >= 1024 ? `${(mb / 1024).toFixed(2)} GB` : `${mb.toFixed(1)} MB`;
	}

	onMount(() => {
		refresh();
		nacistDb();
		const t = setInterval(refresh, 2000);
		// Vlastní tikot pro 'naposledy zjištěno' — bez něj by text
		// zamrzl na hodnotě z posledního překreslení.
		const tik = setInterval(() => (ted = Date.now()), 1000);
		return () => {
			clearInterval(t);
			clearInterval(tik);
		};
	});
</script>

<div class="settings">
	<!-- Nastavení je seznam, ne sada esejí.
	     Každý řádek má jméno, jednu větu proč a ovládání vpravo. Delší
	     vysvětlení, která tu byla dřív, natáhla stránku tak, že se v ní
	     nedalo najít, co vlastně jde přepnout. -->

	<section class="card">
		<header class="card-head"><span class="label-tech">// zobrazení</span></header>

		<button
			class="row opt"
			role="switch"
			aria-checked={prefs.showZeroByte}
			onclick={() => setPref('showZeroByte', !prefs.showZeroByte)}
		>
			<span class="row-main">
				<span class="row-name">Prázdné soubory ve Files</span>
				<span class="row-why">Soubory o 0 B bývají zámky a rozdělaná stahování, ne smetí.</span>
			</span>
			<span class="sw" class:on={prefs.showZeroByte}><span class="knob"></span></span>
		</button>

		<button
			class="row opt"
			role="switch"
			aria-checked={prefs.showSystemStartup}
			onclick={() => setPref('showSystemStartup', !prefs.showSystemStartup)}
		>
			<span class="row-main">
				<span class="row-name">Startovací položky Windows</span>
				<span class="row-why">Jen k náhledu — přepnout je Winsent nedovolí.</span>
			</span>
			<span class="sw" class:on={prefs.showSystemStartup}><span class="knob"></span></span>
		</button>
	</section>

	<section class="card">
		<header class="card-head"><span class="label-tech">// verze a aktualizace</span></header>

		<div class="row">
			<span class="row-main">
				<span class="row-name">Verze</span>
				<span class="row-why">
					{#if updater.available}
						Nová verze je připravená. Aktualizace zavře aplikaci, přepíše soubory
						a spustí ji znovu.
					{:else if updater.error}
						Zjistit verzi se nepodařilo: {updater.error}
					{:else if updater.current}
						Máš aktuální verzi. Kontroluje se každých 30 sekund.
					{:else}
						Běží z vývojového stromu — aktualizace se tu nenabízí.
					{/if}
				</span>
			</span>
			<span class="row-act ver-vals" class:flash={justChecked}>
				<span class="value-mono ver-now">{updater.current || '—'}</span>
				{#if updater.available}
					<span class="ver-arrow">→</span>
					<span class="value-mono ver-new">{updater.latest || '—'}</span>
				{/if}
			</span>
		</div>

		<div class="row">
			<span class="row-main">
				<span class="row-name">Kontrola</span>
				<span class="row-why">
					{#if updater.checking}
						Ptám se repozitáře…
					{:else}
						Naposledy {lastCheck}.
					{/if}
				</span>
			</span>
			<span class="row-act">
				<button class="v-btn" disabled={updater.checking} onclick={checkUpdate}>
					<RefreshCw size={14} class={updater.checking ? 'pc-spin' : ''} />
					{updater.checking ? 'kontroluji…' : 'Zkontrolovat teď'}
				</button>
				{#if updater.available}
					<button class="v-btn primary" disabled={updater.busy} onclick={runUpdate}>
						{updater.busy ? 'stahuji…' : 'Aktualizovat'}
					</button>
				{/if}
			</span>
		</div>
		{#if updater.runError}
			<p class="row-err">{updater.runError}</p>
		{/if}
	</section>

	<section class="card">
		<header class="card-head"><span class="label-tech">// databáze</span></header>

		<div class="row">
			<span class="row-main">
				<span class="row-name">Umístění</span>
				<span class="row-why value-mono path">{db?.current_path ?? '—'}</span>
			</span>
			<span class="row-act">
				<button class="v-btn" disabled={dbBusy} onclick={vybratSlozku}>
					<FolderOpen size={14} /> Změnit
				</button>
				{#if db?.wanted_dir}
					<button class="v-btn" disabled={dbBusy} onclick={() => ulozitSlozku('')}>
						Výchozí
					</button>
				{/if}
			</span>
		</div>

		<div class="row">
			<span class="row-main">
				<span class="row-name">Velikost</span>
				<span class="row-why">Historie měření za posledních 30 dnů.</span>
			</span>
			<span class="row-act value-mono">
				{#if db}
					<Num value={db.bytes} format={fmtBytes} />
					<span class="row-sub">volno <Num value={db.free_bytes} format={fmtBytes} /></span>
				{:else}
					—
				{/if}
			</span>
		</div>

		{#if dbErr}
			<p class="row-err">{dbErr}</p>
		{:else if db?.move_error}
			<!-- Přesun se při startu služby nepovedl. Bez tohohle by v UI
			     donekonečna svítilo „čeká na restart" a jediné, co by
			     o problému vědělo, byl by log. -->
			<p class="row-err">Přesun se nepovedl: {db.move_error}</p>
		{:else if db?.pending}
			<!-- Databáze je otevřená, takže se stěhuje až při startu služby.
			     Říct to nahlas je důležitější než to schovat: jinak uživatel
			     uvidí starou cestu a bude si myslet, že se nic neuložilo. -->
			<div class="row">
				<span class="row-main">
					<span class="row-name">Čeká na restart služby</span>
					<span class="row-why">
						Přesun do <span class="value-mono">{db.wanted_dir}</span> proběhne, až se
						služba znovu spustí — teď je databáze otevřená.
					</span>
				</span>
				<span class="row-act">
					<button class="v-btn primary" disabled={dbBusy} onclick={restartSluzby}>
						<RefreshCw size={14} /> Restartovat službu
					</button>
				</span>
			</div>
		{:else if dbMsg}
			<p class="row-ok">{dbMsg}</p>
		{/if}
	</section>

	<section class="card">
		<header class="card-head"><span class="label-tech">// záznam o počítači</span></header>

		<div class="row">
			<span class="row-main">
				<span class="row-name">Stáhnout záznam</span>
				<span class="row-why">
					Kompletní stav počítače a záznamy za 24 h do textového souboru — k analýze
					člověkem i modelem.
				</span>
			</span>
			<span class="row-act">
				<button class="v-btn primary" disabled={pcState === 'busy'} onclick={downloadPcReport}>
					{#if pcState === 'busy'}
						<RefreshCw size={14} class="pc-spin" /> {pcStep}…
					{:else}
						<Download size={14} /> Stáhnout
					{/if}
				</button>
			</span>
		</div>
		<p class="row-why priv">
			<ShieldCheck size={14} />
			<span>
				Bez obsahu disku: žádné cesty ani seznamy složek. Uloží se do Stažených
				souborů a nikam se neodesílá.
			</span>
		</p>
		{#if pcState === 'done'}
			<p class="row-ok path">Uloženo: {pcPath}</p>
		{:else if pcState === 'error'}
			<p class="row-err">Nepodařilo se: {pcStep}</p>
		{/if}
	</section>

	<section class="card">
		<header class="card-head"><span class="label-tech">// spotřeba nástroje</span></header>
		<p class="row-why">
			Démon se měří stejným samplerem jako každý jiný proces. Rozpočet: &lt; 0,5 % CPU,
			&lt; 50 MB RAM.
		</p>
		{#if usage}
			<div class="tiles">
				<div class="tile">
					<span class="label-tech">cpu</span>
					<span class="val value-mono" class:over={usage.cpu_pct > 0.5}>
						<Num value={usage.cpu_pct} decimals={2} suffix=" %" />
					</span>
				</div>
				<div class="tile">
					<span class="label-tech">ram</span>
					<span class="val value-mono" class:over={usage.ws_bytes > 50 * 1024 * 1024}>
						<Num value={usage.ws_bytes} format={fmtBytes} />
					</span>
				</div>
				<div class="tile">
					<span class="label-tech">databáze</span>
					<span class="val value-mono"><Num value={usage.db_bytes} format={fmtBytes} /></span>
				</div>
			</div>
		{:else}
			<p class="empty label-tech">{daemon.alive ? error || 'čekám na vzorek…' : 'služba neběží'}</p>
		{/if}
		<p class="row-why faint">
			Konfigurace: <span class="value-mono">%ProgramData%\syswatch\config.toml</span>
		</p>
	</section>
</div>

<style>
	.settings {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}
	.card {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 0.9rem 1rem;
	}
	.card-head {
		margin-bottom: 0.6rem;
	}
	/* ── Řádek nastavení ─────────────────────────────────────────
	   Jméno, jedna věta proč, ovládání vpravo. Řádky v kartě sedí
	   pod sebou a dělí je tenká linka, takže to čte jako seznam,
	   ne jako sada odstavců. */
	.row {
		display: flex;
		align-items: center;
		gap: 1rem;
		width: 100%;
		padding: 0.6rem 0;
		border: 0;
		border-top: 1px solid var(--border);
		background: none;
		color: var(--text);
		font: inherit;
		text-align: left;
	}
	.row:first-of-type {
		border-top: 0;
		padding-top: 0.1rem;
	}
	.row-main {
		display: flex;
		flex-direction: column;
		gap: 0.12rem;
		min-width: 0;
		flex: 1;
	}
	.row-name {
		font-size: var(--fs-lg);
		line-height: 1.3;
	}
	.row-why {
		font-size: var(--fs-sm);
		color: var(--text-dim);
		line-height: 1.4;
		margin: 0;
	}
	.row-act {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		flex: none;
		white-space: nowrap;
	}
	.row-sub {
		font-size: var(--fs-xs);
		color: var(--text-faint);
		margin-left: 0.4rem;
	}
	.row-err {
		margin: 0.5rem 0 0;
		font-size: var(--fs-sm);
		color: var(--danger);
		overflow-wrap: anywhere;
	}
	.row-ok {
		margin: 0.5rem 0 0;
		font-size: var(--fs-sm);
		color: var(--ok);
		overflow-wrap: anywhere;
	}
	/* Cesta se smí zalomit kdekoli — jinak roztáhne celou kartu. */
	.path {
		overflow-wrap: anywhere;
	}
	.priv {
		display: flex;
		align-items: flex-start;
		gap: 0.45rem;
		margin-top: 0.6rem;
	}
	.faint {
		margin-top: 0.7rem;
		color: var(--text-faint);
	}
	/* Přepínatelný řádek je celý tlačítko — mířit na malý obdélníček
	   je zbytečná práce navíc. */
	.opt {
		cursor: pointer;
	}
	.opt:hover .row-name {
		color: var(--accent, var(--text));
	}
	/* Verze: současná, a když je co, i ta nová za šipkou. */
	.ver-vals {
		gap: 0.35rem;
	}
	.ver-now {
		color: var(--text-dim);
	}
	.ver-arrow {
		color: var(--text-faint);
	}
	.ver-new {
		color: var(--ok);
	}
	/* Krátké bliknutí po dokončení kontroly.
	   Kontrola běží každých 30 s a většinou nic nezmění, takže bez
	   téhle odezvy nešlo poznat, že tlačítko vůbec něco udělalo. */
	.ver-vals.flash {
		animation: ver-flash 1s ease-out;
	}
	@keyframes ver-flash {
		0% {
			background: color-mix(in srgb, var(--ok) 30%, transparent);
			box-shadow: 0 0 0 4px color-mix(in srgb, var(--ok) 18%, transparent);
			border-radius: 4px;
		}
		100% {
			background: transparent;
			box-shadow: none;
			border-radius: 4px;
		}
	}
	/* Řádek s přepínačem. Celý řádek je tlačítko — mířit na malý
	   obdélníček je zbytečná práce navíc. */
	.opt {
		display: flex;
		align-items: flex-start;
		gap: 0.8rem;
		width: 100%;
		padding: 0.7rem 0.8rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		color: var(--text);
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.opt:hover {
		background: var(--surface-hover);
	}
	/* Přepínač — geometrie 1:1 s tím v Po spuštění, ať je to napříč
	   aplikací tentýž prvek, ne dva podobné. */
	.sw {
		position: relative;
		flex: none;
		width: 38px;
		height: 21px;
		margin-top: 1px;
		border-radius: 999px;
		border: 1px solid var(--border-strong);
		background: var(--panel);
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
	/* Verze: tři údaje vedle sebe, popisky ve stejném jazyce jako
	   jinde v aplikaci (mono verzálky). */
	/* Poznámka o soukromí — modrý štít, ne varování. Je to ujištění,
	   ne problém. */
	.v-btn :global(svg) {
		vertical-align: -2px;
		margin-right: 5px;
	}
	:global(.pc-spin) {
		animation: pc-spin 1.1s linear infinite;
	}
	@keyframes pc-spin {
		to {
			transform: rotate(360deg);
		}
	}
	.v-btn {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 6px 13px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--surface);
		color: var(--text-dim);
		font: inherit;
		font-size: var(--fs-sm);
		cursor: pointer;
		white-space: nowrap;
	}
	.v-btn:disabled {
		opacity: 0.6;
		cursor: default;
	}
	.v-btn:hover:not(:disabled) {
		background: var(--surface-hover);
		color: var(--text);
	}
	.v-btn.primary {
		background: var(--warn);
		border-color: var(--warn);
		color: #1b1200;
		font-weight: 600;
	}
	.v-btn.primary:hover:not(:disabled) {
		filter: brightness(1.12);
	}
	.v-btn:disabled {
		opacity: 0.6;
		cursor: wait;
	}
	.tiles {
		display: flex;
		gap: 0.8rem;
	}
	.tile {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
		min-width: 130px;
		padding: 0.7rem 0.9rem;
		border: 1px dotted var(--border-strong);
		border-radius: var(--radius);
	}
	.tile .val {
		font-size: 1.15rem;
		color: var(--accent);
	}
	/* Překročený rozpočet — jediný význam, kdy tu smí být barva. */
	.tile .val.over {
		color: var(--warn);
	}
	.empty {
		margin: 0.6rem 0;
	}
</style>
