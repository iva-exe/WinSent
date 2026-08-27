<script>
	// Kontextové menu položky. Vykresluje se JEDNOU v layoutu; obsah mu
	// dodává sekce přes `openMenu` (viz itemmenu.svelte.js).
	//
	// Vzhled: stejný podklad jako aplikace, ale silněji rozostřený, aby
	// bylo poznat, že leží nad obsahem, a se zaoblenými rohy.
	import { itemMenu, closeMenu } from '$lib/itemmenu.svelte.js';
	import {
		HelpCircle,
		Copy,
		FolderOpen,
		ExternalLink,
		Search,
		Trash2,
		XCircle,
		Info,
		ShieldCheck,
		Power,
		FileText,
		Globe,
		Package,
		Cpu,
		HardDrive
	} from 'lucide-svelte';

	// Jméno ikony → komponenta. Sekce posílají jen jméno, aby si
	// nemusely tahat lucide do každé stránky.
	const IKONY = {
		help: HelpCircle,
		copy: Copy,
		folder: FolderOpen,
		open: ExternalLink,
		search: Search,
		trash: Trash2,
		kill: XCircle,
		info: Info,
		shield: ShieldCheck,
		power: Power,
		file: FileText,
		web: Globe,
		app: Package,
		cpu: Cpu,
		disk: HardDrive
	};

	let el = $state(null);

	// Menu se nesmí vysunout z okna. Pozice se dopočítá až po
	// vykreslení, kdy známe jeho skutečnou velikost.
	let pos = $state({ x: 0, y: 0 });
	$effect(() => {
		if (!itemMenu.open) return;
		const w = el?.offsetWidth ?? 240;
		const h = el?.offsetHeight ?? 200;
		const okraj = 8;
		pos = {
			x: Math.min(itemMenu.x, window.innerWidth - w - okraj),
			y: Math.min(itemMenu.y, window.innerHeight - h - okraj)
		};
	});

	// Zavírá se čímkoli, co není klik uvnitř: Escape, klik jinam,
	// scroll, změna velikosti okna. Otevřené menu, které přežije
	// odscrollování obsahu, ukazuje na nic.
	$effect(() => {
		if (!itemMenu.open) return;
		const zavri = () => closeMenu();
		const klavesa = (e) => {
			if (e.key === 'Escape') closeMenu();
		};
		window.addEventListener('resize', zavri);
		window.addEventListener('blur', zavri);
		window.addEventListener('keydown', klavesa);
		// `capture`, ať se zavře i při scrollu uvnitř seznamu.
		window.addEventListener('scroll', zavri, true);
		return () => {
			window.removeEventListener('resize', zavri);
			window.removeEventListener('blur', zavri);
			window.removeEventListener('keydown', klavesa);
			window.removeEventListener('scroll', zavri, true);
		};
	});

	async function spust(polozka) {
		if (polozka.disabled) return;
		closeMenu();
		try {
			await polozka.run?.();
		} catch (e) {
			// Selhání akce nesmí zůstat bez odezvy, ale menu už je pryč —
			// sekce si výsledek hlásí sama, tady zbývá jen konzole.
			console.error('akce z kontextového menu selhala:', e);
		}
	}
</script>

{#if itemMenu.open}
	<!-- Podklad chytá klik i pravý klik mimo menu. Bez něj by druhý
	     pravý klik otevřel druhé menu vedle prvního. -->
	<div
		class="backdrop"
		role="presentation"
		onclick={closeMenu}
		oncontextmenu={(e) => {
			e.preventDefault();
			closeMenu();
		}}
	></div>
	<div
		class="menu"
		bind:this={el}
		style:left={`${pos.x}px`}
		style:top={`${pos.y}px`}
		role="menu"
		tabindex="-1"
	>
		{#if itemMenu.title}
			<div class="head">
				<span class="h-title">{itemMenu.title}</span>
				{#if itemMenu.subtitle}
					<span class="h-sub">{itemMenu.subtitle}</span>
				{/if}
			</div>
		{/if}
		<ul class="items">
			{#each itemMenu.items as p, i (i)}
				{@const Ikona = IKONY[p.icon] ?? Info}
				{#if p.separator}
					<li class="sep" role="separator"></li>
				{:else}
					<li>
						<button
							class="mi"
							class:danger={p.danger}
							disabled={p.disabled}
							role="menuitem"
							onclick={() => spust(p)}
						>
							<Ikona size={15} />
							<span class="mi-label">{p.label}</span>
							{#if p.hint}
								<span class="mi-hint">{p.hint}</span>
							{/if}
						</button>
					</li>
				{/if}
			{/each}
		</ul>
	</div>
{/if}

<style>
	.backdrop {
		position: fixed;
		inset: 0;
		z-index: 900;
	}
	/* Stejný podklad jako aplikace, jen silněji rozostřený — menu má
	   být poznat jako vrstva nad obsahem, ne jako další karta. */
	.menu {
		position: fixed;
		z-index: 901;
		min-width: 220px;
		max-width: 340px;
		padding: 5px;
		border: 1px solid var(--border-strong, var(--border));
		border-radius: var(--radius-lg);
		/* Podklad se bere z `--bg`, ne z `--surface`.
		   `--surface` je světlý závoj (bílá na 4 %), takže menu z něj
		   vycházelo skoro průhledné a text pod ním prosvítal.
		   Za menu nemá být nic čitelného: samotné rozostření na to
		   nestačí, velký text zůstane rozpoznatelný i při 40 px, proto
		   nese hlavní práci krytí a rozostření jen změkčuje okraje. */
		background: color-mix(in srgb, var(--bg) 97%, transparent);
		backdrop-filter: blur(40px) saturate(150%);
		-webkit-backdrop-filter: blur(40px) saturate(150%);
		box-shadow:
			0 12px 32px rgba(0, 0, 0, 0.45),
			0 2px 8px rgba(0, 0, 0, 0.3);
		animation: mi-in 0.09s ease-out;
	}
	@keyframes mi-in {
		from {
			opacity: 0;
			transform: translateY(-3px) scale(0.985);
		}
	}
	.head {
		display: flex;
		flex-direction: column;
		gap: 1px;
		padding: 6px 9px 7px;
		border-bottom: 1px solid var(--border);
		margin-bottom: 4px;
	}
	.h-title {
		font-size: var(--fs-sm);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.h-sub {
		font-size: var(--fs-2xs);
		color: var(--text-faint);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.items {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.sep {
		height: 1px;
		margin: 4px 6px;
		background: var(--border);
	}
	.mi {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		padding: 6px 9px;
		border: 0;
		border-radius: var(--radius-sm);
		background: none;
		color: var(--text-dim);
		font: inherit;
		font-size: var(--fs-sm);
		text-align: left;
		cursor: pointer;
	}
	.mi:hover:not(:disabled) {
		background: var(--surface-hover);
		color: var(--text);
	}
	.mi:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.mi.danger:hover:not(:disabled) {
		color: var(--danger);
	}
	.mi-label {
		flex: none;
	}
	/* Co se vlastně vyhledá — ať uživatel neklikne naslepo. */
	.mi-hint {
		flex: 1;
		min-width: 0;
		text-align: right;
		font-size: var(--fs-2xs);
		color: var(--text-faint);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
