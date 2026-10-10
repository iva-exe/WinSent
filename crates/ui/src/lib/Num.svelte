<script module>
	// Sdílené hodiny všech <Num>.
	//
	// Dřív měl každý <Num> vlastní Tween ze svelte/motion a ten přepisoval
	// text v KAŽDÉM snímku obrazovky. Na 175Hz monitoru to po každém
	// vzorku dělalo kolem šedesáti překreslení okna za sekundu jen kvůli
	// doběhu čísel (naměřeno: GPU proces WebView2 pracuje ~300 ms po
	// každém vzorku, pak stojí). Číslice, které se střídají rychleji než
	// ~60× za sekundu, oko stejně nerozliší.
	//
	// Proto: jeden rAF pro všechna čísla a text se posune nejvýš ~60×
	// za sekundu, vždy ve stejném snímku pro všechna čísla naráz. Křivka
	// (cubicOut, 500 ms) i výchozí bod jsou stejné jako u Tween; na 60Hz
	// monitoru se nemění nic, na 175 Hz se kreslí každý třetí snímek.
	import { cubicOut } from 'svelte/easing';

	const DELKA = 500;
	// 14,7 ms: 175 Hz → každý 3. snímek (58 Hz), 144 Hz → každý 3.,
	// 120 Hz → každý 2., 60 Hz → každý. Rezerva 2 ms kvůli kolísání rAF.
	const KROK = 1000 / 60 - 2;

	/**
	 * Křivka animace; `pred` = předchozí křivka, dokud nový cíl ještě
	 * neprošel snímkem (viz spust).
	 * @typedef {{ od: number, cil: number, start: number, pred: Krivka | null }} Krivka
	 */
	/** @typedef {Krivka & { zapis: (v: number) => void }} Anim */
	/** @type {Set<Anim>} */
	const bezici = new Set();
	let raf = 0;
	// Čas snímku (argument rAF), ve kterém se naposledy psal text.
	let posledni = -Infinity;

	function snimek(/** @type {number} */ casSnimku) {
		raf = 0;
		// Dvoje hodiny, každé na něco jiného. KDY psát rozhoduje čas
		// snímku z rAF — je zarovnaný na vsync a stejný pro celý snímek,
		// takže rytmus „každý třetí" drží i ve chvíli, kdy se callback
		// opozdí za obsluhou myši. KDE na křivce být určuje
		// performance.now(), stejně jako u Tween: start se bere taky
		// z něj a čas snímku může být o kousek starší než start, takže by
		// křivka na prvním snímku couvla pod výchozí bod.
		const ted = performance.now();
		let dobehlo = false;
		for (const a of bezici) {
			// I ve snímku, kdy se nepíše — Tween bere výchozí bod
			// v prvním snímku po novém cíli, ne až při dalším zápisu.
			navaz(a, ted);
			if (ted - a.start >= DELKA) dobehlo = true;
		}
		// Konec animace se nikdy neodkládá — cílová hodnota musí sedět
		// přesně a včas, jinak by číslo „dobíhalo" o snímek déle.
		if (dobehlo || casSnimku - posledni >= KROK) {
			posledni = casSnimku;
			for (const a of bezici) {
				a.zapis(bod(a, ted));
				if (ted - a.start >= DELKA) bezici.delete(a);
			}
		}
		if (bezici.size) raf = requestAnimationFrame(snimek);
	}

	/** Hodnota křivky v čase `ted`; po doběhu přesně cíl. */
	function bod(/** @type {Krivka} */ k, /** @type {number} */ ted) {
		navaz(k, ted);
		const t = (ted - k.start) / DELKA;
		return t >= 1 ? k.cil : k.od + (k.cil - k.od) * cubicOut(t);
	}

	/** Výchozí bod nového cíle = předchozí křivka v čase snímku. */
	function navaz(/** @type {Krivka} */ k, /** @type {number} */ ted) {
		// Víc cílů před jediným snímkem (skryté okno, vzorek a k tomu
		// najetí myší) dělá řetěz; Tween ho rozplétá stejně, úlohu po úloze.
		if (k.pred) {
			k.od = bod(k.pred, ted);
			k.pred = null;
		}
	}

	function spust(/** @type {Anim} */ a, /** @type {number} */ cil) {
		const ted = performance.now();
		if (bezici.has(a) && ted - a.start < DELKA) {
			// Stará křivka ještě běží. Výchozí bod se NEurčuje teď:
			// Tween ho čte až v prvním snímku po nastavení, kdy stará
			// úloha v témž snímku už posunula hodnotu na čas snímku (start
			// přitom drží čas nastavení). Do snímku se proto stará křivka
			// jen odloží a navaz() z ní bod dopočte.
			a.pred = { od: a.od, cil: a.cil, start: a.start, pred: a.pred };
		} else {
			// Doběhlá křivka končí přesně v cíli, ať snímek přijde kdykoli —
			// tady se navazuje hned a řetěz neroste (ani při oknu skrytém
			// celé hodiny, kdy snímky stojí a vzorky chodí dál).
			a.od = a.cil;
			a.pred = null;
		}
		a.cil = cil;
		a.start = ted;
		bezici.add(a);
		if (!raf) raf = requestAnimationFrame(snimek);
	}

	function zastav(/** @type {Anim} */ a) {
		bezici.delete(a);
	}
</script>

<script>
	// Animované číslo — hodnota plyne místo skoku. Používá se napříč
	// aplikací u všech proměnných KROMĚ řádků v listu procesů (tam by
	// stovky animací škodily výkonu).
	let { value = null, decimals = 1, suffix = '', format = null } = $props();

	// Jako Tween(0): první hodnota se „načítá" od nuly.
	let zobrazeno = $state(0);
	/** @type {Anim} */
	const anim = { od: 0, cil: 0, start: 0, pred: null, zapis: (v) => (zobrazeno = v) };

	$effect(() => {
		const v = value;
		// Stejný cíl = nic. Tween dřív rozjel 500 ms snímků i pro
		// hodnotu, která se nezměnila (teplota, počet procesů…), protože
		// efekt se spouští s každým vzorkem. NaN se sám sobě nerovná —
		// bez druhé podmínky by se kvůli němu rozjížděla prázdná animace
		// každou sekundu.
		if (v == null || v === anim.cil || (Number.isNaN(v) && Number.isNaN(anim.cil))) return;
		spust(anim, v);
	});

	// Při odchodu ze stránky se animace odhlásí; poslední číslo tak
	// nedrží sdílené hodiny v chodu.
	$effect(() => () => zastav(anim));

	const text = $derived(
		value == null ? '—' : format ? format(zobrazeno) : `${zobrazeno.toFixed(decimals)}${suffix}`
	);
</script>

{text}
