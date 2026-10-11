<script module>
	// Barvy z :root si pamatují všechny sparkliny společně. Dřív se při
	// každém překreslení pětkrát volal getComputedStyle (a s ním
	// přepočet stylů) — v Tasks jich je dvanáct a kreslí se s každým
	// vzorkem.
	//
	// Za běhu se barvy nemění: app.css je má na :root napevno a nic je
	// nepřepíná (žádné téma, prefers-color-scheme ani setProperty).
	// Prázdná hodnota (styly ještě nenačtené) se nepamatuje, takže se
	// i ten případ chová přesně jako dřív. Kdyby jednou přibylo
	// přepínání vzhledu, musí se tahle paměť při přepnutí vyprázdnit.
	/** @type {Map<string, string>} */
	const barvy = new Map();

	function cssVar(/** @type {string} */ name) {
		let v = barvy.get(name);
		if (v === undefined) {
			v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
			if (v) barvy.set(name, v);
		}
		return v;
	}
</script>

<script>
	// Mini graf zátěže (canvas, bez knihovny) — pro jádra CPU v detail
	// sekci. Škála pevně 0–100 %. Barva čáry = stejný vertikální
	// gradient podle zátěže jako hlavní graf (zelená dole → červená
	// nahoře), takže výška bodu odpovídá jeho barvě.
	// `marker` = index bodu, na kterém se vykreslí svislá linka (zámek
	// času) — okno dat se předává tak, aby byl bod uprostřed.
	// `barva` (např. 'var(--net-down)') nahradí gradient jednou barvou
	// pro křivky, které zátěž nejsou: síť nemá strop, a gradient by
	// každou špičku okna maloval červeně, i kdyby šlo o pár kB/s.
	import { GRADIENT_ZATEZE } from '$lib/barvy.js';

	let { values = [], height = 26, marker = null, barva = null } = $props();

	// Náhradní barvy, kdyby styly ještě nebyly načtené (hodnoty z app.css).
	const NAHRADNI = { ok: '#4ade80', warn: '#f59e0b', danger: '#ef4444' };

	let canvas;

	$effect(() => {
		// závislosti: values, marker
		values;
		marker;
		if (!canvas) return;
		const dpr = window.devicePixelRatio || 1;
		const w = canvas.clientWidth;
		const h = height;
		if (w === 0) return;
		const ctx = canvas.getContext('2d');
		// Přiřazení width/height zahodí bitmapu i stav kontextu, i když
		// se velikost nezměnila — a kreslí se s každým vzorkem. Nová
		// bitmapa proto jen při skutečné změně velikosti v pixelech
		// (prohlížeč desetinná čísla usekne, proto Math.trunc); jinak
		// stačí vymazat. Stav kontextu kromě transformace se nevrací:
		// kreslení níž si barvu, tloušťku i cestu pokaždé nastaví samo.
		if (canvas.width !== Math.trunc(w * dpr) || canvas.height !== Math.trunc(h * dpr)) {
			canvas.width = w * dpr;
			canvas.height = h * dpr;
		} else {
			ctx.resetTransform();
			ctx.clearRect(0, 0, canvas.width, canvas.height);
		}
		ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		if (values.length < 2) return;

		let g;
		if (barva) {
			// Plná barva; `var(--x)` se čte z :root přes tutéž paměť.
			const b = String(barva).trim();
			g = b.startsWith('var(') ? cssVar(b.slice(4, -1).trim()) || '#888' : b;
		} else {
			// Gradient odspoda (0 %) nahoru (100 %) — zastávky z $lib/barvy.js,
			// tytéž jako hlavní graf a malé grafy na Home.
			g = ctx.createLinearGradient(0, h, 0, 0);
			for (const [poloha, jmeno] of GRADIENT_ZATEZE) {
				g.addColorStop(poloha, cssVar(`--${jmeno}`) || NAHRADNI[jmeno]);
			}
		}

		ctx.beginPath();
		const n = values.length;
		for (let i = 0; i < n; i++) {
			const x = (i / (n - 1)) * w;
			const y = h - (Math.min(values[i] ?? 0, 100) / 100) * (h - 2) - 1;
			if (i === 0) ctx.moveTo(x, y);
			else ctx.lineTo(x, y);
		}
		ctx.strokeStyle = g;
		ctx.lineWidth = 1.4;
		ctx.stroke();

		// Linka zámku času.
		if (marker != null && marker >= 0 && marker < n) {
			const mx = (marker / (n - 1)) * w;
			ctx.beginPath();
			ctx.moveTo(mx, 0);
			ctx.lineTo(mx, h);
			ctx.strokeStyle = 'rgba(255, 255, 255, 0.55)';
			ctx.lineWidth = 1;
			ctx.stroke();
		}
	});
</script>

<canvas bind:this={canvas} style:height={`${height}px`}></canvas>

<style>
	canvas {
		width: 100%;
		display: block;
	}
</style>
