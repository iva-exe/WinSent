<script module>
	// Barvy z :root (`var(--ok)` a spol.) si pamatují všechny grafy
	// společně — getComputedStyle by při každém překreslení vynutil
	// přepočet stylů. Za běhu se nemění: app.css je má na :root napevno
	// a nic je nepřepíná (žádné téma, prefers-color-scheme ani
	// setProperty). Prázdná hodnota (styly ještě nenačtené) se
	// nepamatuje, takže i tehdy platí náhradní šedá jako dřív. Kdyby
	// jednou přibylo přepínání vzhledu, musí se tahle paměť při
	// přepnutí vyprázdnit.
	/** @type {Map<string, string>} */
	const barvyRoot = new Map();
</script>

<script>
	// Malý průběh do dlaždice (canvas, bez knihovny).
	//
	// Proč ne LiveChart z Tasks: ten má na kolečku vlastní obsluhu
	// s preventDefault, takže by rolování myší nad dlaždicí zastavilo
	// posun celého přehledu. Widget navíc žádný pan ani zoom nechce —
	// je to náhled, ne nástroj.
	//
	// Škála: procentní režim má pevných 0–100 (jinak by 3% špička
	// vypadala jako vytížený stroj), síťový režim se přizpůsobuje
	// maximu v okně, protože absolutní strop u linky neexistuje.
	import { GRADIENT_ZATEZE } from '$lib/barvy.js';

	let {
		/// Hodnoty zleva doprava; poslední je „teď". `null` = metrika,
		/// kterou stroj v tom vzorku nehlásil.
		values = [],
		/// Druhá série (upload) — kreslí se slabší barvou.
		values2 = null,
		/// 'pct' = pevná osa 0–100, 'auto' = podle maxima.
		skala = 'pct',
		barva = 'var(--ok)',
		barva2 = 'var(--net-up)',
		/// Výška v bodech; `null` = vyplnit, co dá rodič (dlaždice se
		/// dá natáhnout, takže pevná čísla by se rozešla).
		vyska = 44,
		/// Vyplnit plochu pod křivkou?
		vypln = true,
		/// Série je zátěž v procentech (CPU, RAM, GPU, systém): čára
		/// i výplň v gradientu podle zátěže jako grafy v Tasks, `barva`
		/// se nepoužije. Jedna barva na metriku dřív říkala něco jiného
		/// než výška křivky — grafika byla oranžová i v klidu. Jen pro
		/// skala 'pct': gradient leží na pevné ose 0–100.
		zatez = false
	} = $props();

	let canvas;
	// Dlaždice se dá natáhnout za hranu a plátno se samo nepřekreslí —
	// do dalšího vzorku by zůstala roztažená stará bitmapa. Tohle je ta
	// jediná závislost, kterou nejde vyčíst z dat.
	let zmenaVelikosti = $state(0);

	$effect(() => {
		if (!canvas) return;
		const ro = new ResizeObserver(() => (zmenaVelikosti += 1));
		ro.observe(canvas);
		return () => ro.disconnect();
	});

	function cssBarva(v) {
		const s = String(v).trim();
		if (!s.startsWith('var(')) return s;
		const jmeno = s.slice(4, -1).trim();
		let b = barvyRoot.get(jmeno);
		if (b === undefined) {
			b = getComputedStyle(document.documentElement).getPropertyValue(jmeno).trim();
			if (b) barvyRoot.set(jmeno, b);
		}
		return b || '#888';
	}

	// Gradient zátěže přes osu 0–100 %: od y hodnoty 0 (h − 1) k y
	// hodnoty 100 (1), stejný převod jako u bodů křivky — barva čáry
	// v každém místě odpovídá její výšce. Zastávky z $lib/barvy.js,
	// průhlednost jako v LiveChart (čára plná, výplň 0.07).
	function gradientZateze(ctx, h, alfa) {
		const g = ctx.createLinearGradient(0, h - 1, 0, 1);
		for (const [poloha, jmeno] of GRADIENT_ZATEZE) {
			g.addColorStop(poloha, sAlfou(cssBarva(`var(--${jmeno})`), alfa));
		}
		return g;
	}

	// Hex z app.css (i tříznakový náhradní „#888") jako rgba. Připojit
	// průhlednost za hex nejde u „#888" — z pěti znaků by byla neplatná
	// barva a addColorStop by vyhodil výjimku.
	function sAlfou(hex, alfa) {
		if (alfa >= 1) return hex;
		const v = hex.replace('#', '');
		const n = parseInt(v.length === 3 ? v.replace(/./g, (c) => c + c) : v, 16);
		return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alfa})`;
	}

	// `barvaCss` = null → gradient zátěže místo jedné barvy.
	function krivka(ctx, data, w, h, max, barvaCss, plnit) {
		if (data.length < 2) return;
		const dx = w / (data.length - 1);
		ctx.beginPath();
		data.forEach((v, i) => {
			const y = h - Math.min((v ?? 0) / max, 1) * (h - 2) - 1;
			if (i === 0) ctx.moveTo(0, y);
			else ctx.lineTo(i * dx, y);
		});
		ctx.strokeStyle = barvaCss ?? gradientZateze(ctx, h, 1);
		ctx.lineWidth = 1.5;
		ctx.lineJoin = 'round';
		ctx.stroke();
		if (!plnit) return;
		ctx.lineTo(w, h);
		ctx.lineTo(0, h);
		ctx.closePath();
		if (barvaCss == null) {
			ctx.fillStyle = gradientZateze(ctx, h, 0.07);
		} else {
			const g = ctx.createLinearGradient(0, 0, 0, h);
			g.addColorStop(0, barvaCss + '38');
			g.addColorStop(1, barvaCss + '00');
			ctx.fillStyle = g;
		}
		ctx.fill();
	}

	$effect(() => {
		// Závislosti: pole se mění na místě, takže se čte i délka
		// a poslední hodnota — jinak by se efekt po push nespustil.
		values.length;
		values[values.length - 1];
		values2?.length;
		skala;
		vyska;
		zmenaVelikosti;
		if (!canvas) return;
		const w = canvas.clientWidth;
		const h = vyska ?? canvas.clientHeight;
		if (!w || !h) return;
		const dpr = window.devicePixelRatio || 1;
		const ctx = canvas.getContext('2d');
		// Přiřazení width/height zahodí bitmapu i stav kontextu, i když
		// se velikost nezměnila. Nová bitmapa proto jen při skutečné
		// změně velikosti v pixelech (prohlížeč desetinná čísla usekne,
		// proto Math.trunc); jinak stačí vymazat. Stav kontextu kromě
		// transformace se nevrací: krivka() si barvu, tloušťku, spoj
		// i cestu pokaždé nastaví sama.
		if (canvas.width !== Math.trunc(w * dpr) || canvas.height !== Math.trunc(h * dpr)) {
			canvas.width = w * dpr;
			canvas.height = h * dpr;
		} else {
			ctx.resetTransform();
			ctx.clearRect(0, 0, canvas.width, canvas.height);
		}
		ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

		let max = 100;
		if (skala === 'auto') {
			const vse = values2 ? [...values, ...values2] : values;
			max = Math.max(1, ...vse.map((v) => v ?? 0)) * 1.15;
		}
		if (values2) krivka(ctx, values2, w, h, max, cssBarva(barva2), false);
		krivka(ctx, values, w, h, max, zatez ? null : cssBarva(barva), vypln);
	});
</script>

<canvas bind:this={canvas} style:height={vyska ? `${vyska}px` : '100%'}></canvas>

<style>
	canvas {
		display: block;
		width: 100%;
	}
</style>
