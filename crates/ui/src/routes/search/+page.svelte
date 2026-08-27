<script>
	// Vyhledávání (sekce) — search bar nahoře, seznam pod ním. Nic víc.
	//
	// Táž komponenta obsluhuje i spotlight lištu na klávesovou zkratku
	// (viz /spotlight). Dvě kopie by se v chování rozešly.
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import FileSearch from '$lib/FileSearch.svelte';
	import { Keyboard } from 'lucide-svelte';

	let zkratka = $state('');

	onMount(async () => {
		try {
			zkratka = await invoke('get_spotlight_hotkey');
		} catch {
			/* hostitel to neumí — jen se nezobrazí nápověda */
		}
	});

	async function otevritListu() {
		try {
			await invoke('show_spotlight');
		} catch {
			/* okno se neotevřelo; zkratka zůstává */
		}
	}
</script>

<div class="page">
	<header class="head">
		<h1>Vyhledávání</h1>
		{#if zkratka}
			<!-- Lišta je hlavní způsob, jak se sem dostat — sekce v aplikaci
			     je spíš pro toho, kdo si na zkratku ještě nezvykl. -->
			<button class="hint" onclick={otevritListu} title="Otevřít vyhledávací lištu">
				<Keyboard size={15} />
				<span>kdekoli ve Windows</span>
				<kbd>{zkratka}</kbd>
			</button>
		{/if}
	</header>

	<div class="body">
		<FileSearch />
	</div>
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.head {
		display: flex;
		align-items: center;
		gap: 0.9rem;
		padding-bottom: 0.7rem;
		flex: none;
	}
	h1 {
		font-size: 1.2rem;
		font-weight: 600;
		margin: 0;
	}
	.hint {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		margin-left: auto;
		padding: 5px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: none;
		color: var(--text-dim);
		font: inherit;
		font-size: var(--fs-sm);
		cursor: pointer;
	}
	.hint:hover {
		background: var(--surface-hover);
		color: var(--text);
	}
	kbd {
		padding: 1px 6px;
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		background: var(--surface);
		font-family: 'Fira Mono', monospace;
		font-size: var(--fs-xs);
	}
	.body {
		flex: 1;
		min-height: 0;
	}
</style>
