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

	let lastCheck = $derived.by(() => {
		if (!updater.checkedAt) return 'zatím ne';
		const s = Math.round((Date.now() - updater.checkedAt) / 1000);
		if (s < 90) return 'před chvílí';
		const m = Math.round(s / 60);
		return m < 90 ? `před ${m} min` : `před ${Math.round(m / 60)} h`;
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
		return () => clearInterval(t);
	});
</script>

<div class="settings">
	<section class="card">
		<header class="card-head">
			<span class="label-tech">// settings / spotřeba nástroje</span>
		</header>
		<p class="note">
			Démon se měří stejným samplerem jako každý jiný proces — žádná výjimka,
			žádné skrývání. Rozpočet: &lt; 0,5&nbsp;% CPU, &lt; 50&nbsp;MB RAM.
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
	</section>



	<section class="card">
		<header class="card-head">
			<span class="label-tech">// settings / zobrazení</span>
		</header>
		<p class="note">
			Co se má v aplikaci ukazovat. Na systém to nesahá — Winsent nic nepřepíná
			ani neskrývá před Windows, je to jen volba zobrazení.
		</p>
		<!-- Přepínač je popiskem i tlačítkem zároveň: klik kamkoli na řádek
		     přepne, aby se nemuselo mířit na malý obdélníček. -->
		<button
			class="opt"
			role="switch"
			aria-checked={prefs.showZeroByte}
			onclick={() => setPref('showZeroByte', !prefs.showZeroByte)}
		>
			<span class="sw" class:on={prefs.showZeroByte}><span class="knob"></span></span>
			<span class="opt-text">
				<span class="opt-name">Ukazovat prázdné soubory ve Files</span>
				<span class="opt-why">
					Soubory o velikosti 0 B nejsou samy o sobě smetí — bývají to dočasné
					soubory aplikací, zámky nebo rozdělaná stahování. Ve výchozím stavu
					se proto neukazují.
				</span>
			</span>
		</button>
		<button
			class="opt"
			role="switch"
			aria-checked={prefs.showSystemStartup}
			onclick={() => setPref('showSystemStartup', !prefs.showSystemStartup)}
		>
			<span class="sw" class:on={prefs.showSystemStartup}><span class="knob"></span></span>
			<span class="opt-text">
				<span class="opt-name">Ukazovat startovací položky Windows</span>
				<span class="opt-why">
					V Po spuštění jsou vidět jen programy třetích stran — to, co s Windows
					startuje ze systému samotného, se nezobrazuje. Přepnout to stejně nejde
					(služba to odmítne) a dlouhý seznam nepřepínatelných řádků jen zakryje
					to, co ovlivnit můžeš. Zapnuté je uvidíš k náhledu, bez přepínače.
				</span>
			</span>
		</button>
	</section>

	<section class="card">
		<header class="card-head">
			<span class="label-tech">// settings / verze a aktualizace</span>
		</header>
		<div class="ver-grid">
			<div>
				<span class="label-tech">nainstalovaná verze</span>
				<span class="ver-val value-mono">{updater.current || '—'}</span>
			</div>
			<div>
				<span class="label-tech">verze k dispozici</span>
				<span class="ver-val value-mono" class:new={updater.available}>
					{updater.latest || '—'}
				</span>
			</div>
			<div>
				<span class="label-tech">naposledy zjištěno</span>
				<span class="ver-val value-mono small">{lastCheck}</span>
			</div>
		</div>
		<p class="note ver-note">
			{#if updater.available}
				Nová verze je připravená. Aktualizace zavře aplikaci i hlídače na pozadí, přepíše
				soubory a spustí to znovu — Windows se cestou zeptají na práva správce.
			{:else if updater.error}
				Zjistit verzi se nepodařilo: {updater.error}
			{:else if updater.current}
				Máš aktuální verzi. Kontroluje se při startu a pak jednou za šest hodin.
			{:else}
				Aplikace neběží z instalace (vývojový strom) — aktualizace se tu nenabízí.
			{/if}
		</p>
		{#if updater.runError}
			<p class="note ver-err">{updater.runError}</p>
		{/if}
		<div class="ver-actions">
			<button class="v-btn" onclick={checkUpdate}>Zkontrolovat teď</button>
			{#if updater.available}
				<button class="v-btn primary" disabled={updater.busy} onclick={runUpdate}>
					{updater.busy ? 'stahuji…' : 'Aktualizovat'}
				</button>
			{/if}
		</div>
	</section>


	<section class="card">
		<header class="card-head">
			<span class="label-tech">// settings / záznam o počítači</span>
		</header>
		<p class="note">
			Textový soubor s kompletním stavem počítače: sestava, hardware, ovladače,
			ochrana, oprávnění, účty, síť, programy, procesy, položky po spuštění
			a všechny záznamy za posledních 24 hodin. Určený k analýze — ať už ho
			čte odborník, nebo model.
		</p>
		<p class="note pc-priv">
			<ShieldCheck size={15} />
			<span>
				Obsah disku v něm <b>není</b>. Žádné cesty k souborům, seznamy složek,
				duplicity ani mapy instalací — z disků jde do záznamu jen technika:
				model, zdraví, teplota, kapacita. Soubor se uloží do Stažených souborů
				a nikam se neodesílá.
			</span>
		</p>
		{#if pcState === 'done'}
			<p class="note pc-done">Uloženo: <span class="value-mono">{pcPath}</span></p>
		{:else if pcState === 'error'}
			<p class="note pc-err">Nepodařilo se: {pcStep}</p>
		{/if}
		<div class="ver-actions">
			<button class="v-btn primary" disabled={pcState === 'busy'} onclick={downloadPcReport}>
				{#if pcState === 'busy'}
					<RefreshCw size={14} class="pc-spin" /> sbírám — {pcStep}…
				{:else}
					<Download size={14} /> Stáhnout záznam o počítači
				{/if}
			</button>
		</div>
	</section>

	<section class="card">
		<header class="card-head">
			<span class="label-tech">// settings / kam se ukládá databáze</span>
		</header>
		<p class="note">
			Historie měření roste do stovek megabajtů. Když máš malý nebo opotřebovaný
			systémový disk, dá se odsunout jinam — výchozí umístění zůstává tam, kde bylo.
		</p>
		{#if dbErr}
			<p class="note pc-err">{dbErr}</p>
		{:else if db}
			<div class="db-rows">
				<div class="db-row">
					<span class="db-k">Teď leží v</span>
					<span class="value-mono db-v">{db.current_path}</span>
				</div>
				<div class="db-row">
					<span class="db-k">Velikost</span>
					<span class="db-v value-mono"><Num value={db.bytes} format={fmtBytes} /></span>
				</div>
				<div class="db-row">
					<span class="db-k">Volné místo</span>
					<span class="db-v value-mono"><Num value={db.free_bytes} format={fmtBytes} /></span>
				</div>
			</div>
			{#if db.pending}
				<!-- Databáze je otevřená, takže se stěhuje až při startu
				     služby. Říct to nahlas je důležitější než to schovat:
				     jinak uživatel uvidí starou cestu a bude si myslet,
				     že se nastavení neuložilo. -->
				<p class="note db-pending">
					Přesun do <span class="value-mono">{db.wanted_dir}</span> čeká na restart služby —
					databáze je teď otevřená a hýbat s ní pod rukama by znamenalo přijít
					o poslední vzorky. Restartuj službu, nebo počítač.
				</p>
			{/if}
			{#if dbMsg}
				<p class="note pc-done">{dbMsg}</p>
			{/if}
			<div class="ver-actions">
				<button class="v-btn" disabled={dbBusy} onclick={vybratSlozku}>
					<FolderOpen size={14} /> Vybrat složku…
				</button>
				{#if db.wanted_dir}
					<button class="v-btn" disabled={dbBusy} onclick={() => ulozitSlozku('')}>
						Zpátky na výchozí
					</button>
				{/if}
				{#if db.pending}
					<button class="v-btn primary" disabled={dbBusy} onclick={restartSluzby}>
						<RefreshCw size={14} /> Restartovat službu
					</button>
				{/if}
			</div>
		{:else}
			<p class="note">Načítám…</p>
		{/if}
	</section>

	<section class="card">
		<header class="card-head">
			<span class="label-tech">// settings / konfigurace</span>
		</header>
		<p class="note">
			Konfigurace žije v <span class="value-mono">%ProgramData%\syswatch\config.toml</span>
			a změny se projeví za běhu (hot-reload). Grafické nastavení přijde s dalšími verzemi.
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
	.note {
		margin: 0 0 0.8rem;
		color: var(--text-dim);
		font-size: var(--fs-lg);
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
	.opt-text {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}
	.opt-name {
		font-size: var(--fs-xl);
	}
	.opt-why {
		font-size: var(--fs-md);
		color: var(--text-dim);
		line-height: 1.5;
	}
	/* Verze: tři údaje vedle sebe, popisky ve stejném jazyce jako
	   jinde v aplikaci (mono verzálky). */
	.ver-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
		gap: 0.8rem;
		margin-bottom: 0.7rem;
	}
	.ver-grid > div {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}
	.ver-val {
		font-size: var(--fs-lg);
		color: var(--text);
	}
	.ver-val.small {
		font-size: var(--fs-sm);
		color: var(--text-dim);
	}
	/* Jantarová jen když je co stáhnout — jinak by to křičelo pořád. */
	.ver-val.new {
		color: var(--warn);
	}
	.ver-note {
		margin: 0 0 0.7rem;
	}
	.ver-err {
		margin: 0 0 0.7rem;
		color: var(--danger);
	}
	/* Poznámka o soukromí — modrý štít, ne varování. Je to ujištění,
	   ne problém. */
	.pc-priv {
		display: flex;
		align-items: flex-start;
		gap: 9px;
		padding: 9px 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
	}
	.pc-priv :global(svg) {
		flex: none;
		margin-top: 2px;
		color: var(--net-down);
	}
	.pc-done {
		color: var(--ok);
		word-break: break-all;
	}
	.pc-err {
		color: var(--danger);
	}
	/* Umístění databáze: hodnoty pod sebou, cesta se smí zalomit. */
	.db-rows {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
		margin-bottom: 0.8rem;
	}
	.db-row {
		display: grid;
		grid-template-columns: 8.5rem minmax(0, 1fr);
		align-items: baseline;
		gap: 0.6rem;
	}
	.db-k {
		font-size: var(--fs-sm);
		color: var(--text-dim);
	}
	.db-v {
		word-break: break-all;
	}
	.db-pending {
		color: var(--warn);
	}
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
	.ver-actions {
		display: flex;
		gap: 0.5rem;
	}
	.v-btn {
		padding: 6px 13px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--surface);
		color: var(--text-dim);
		font: inherit;
		font-size: var(--fs-sm);
		cursor: pointer;
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
