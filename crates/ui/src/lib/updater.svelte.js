// Stav aktualizace pro celou aplikaci.
//
// Bydlí v modulu, ne ve stránce, protože ho potřebují dvě místa naráz:
// trvalé upozornění vpravo dole (kreslí ho layout, takže je vidět
// všude) a dlaždice v Settings, kde se uživatel podívá, co vlastně má.
// Dvě kopie stavu by znamenaly dvě různá čísla na jedné obrazovce.
import { invoke } from '@tauri-apps/api/core';

export const updater = $state({
	/// Verze, která běží. Prázdná = běží se z vývojového stromu.
	current: '',
	/// Verze v repozitáři; prázdná, dokud se nepodařilo zjistit.
	latest: '',
	/// Je co aktualizovat?
	available: false,
	/// Poslední kontrola (ms epocha) — `null` = ještě neproběhla.
	checkedAt: null,
	/// Proč kontrola nevyšla. Upozornění to nekřičí, Settings ano.
	error: '',
	/// Běží stahování instalátoru?
	busy: false,
	/// Chyba spuštění aktualizace.
	runError: ''
});

export async function checkUpdate() {
	try {
		const r = await invoke('check_update');
		updater.current = r.current ?? '';
		updater.latest = r.latest ?? '';
		updater.available = !!r.available;
		updater.error = r.error ?? '';
	} catch (e) {
		updater.error = String(e);
		updater.available = false;
	}
	updater.checkedAt = Date.now();
}

export async function runUpdate() {
	if (updater.busy) return;
	updater.busy = true;
	updater.runError = '';
	try {
		// Instalátor si sám zastaví službu, zavře tohle okno, přepíše
		// binárky a aplikaci zase spustí. Odsud se tedy nedočkáme
		// odpovědi — okno zmizí dřív.
		await invoke('run_update');
	} catch (e) {
		updater.runError = String(e);
		updater.busy = false;
	}
}

let timer;

/// Spustí kontrolu (idempotentní). Při startu hned, pak jednou za šest
/// hodin — častěji nemá smysl, vydává se v řádu dnů.
export function startUpdateChecks() {
	if (timer) return;
	checkUpdate();
	timer = setInterval(checkUpdate, 6 * 3600 * 1000);
}
