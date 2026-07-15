<script>
	// Živý časový graf (uPlot, DESIGN.md kap. 7): primární křivka bílá,
	// sekundární tlumená, tečkovaná mřížka, osy Fira Mono. Gradient jen
	// v tahu křivky — pozadí čisté.
	import uPlot from 'uplot';
	import 'uplot/dist/uPlot.min.css';
	import { onMount } from 'svelte';

	let { ts = [], cpu = [], mem = [] } = $props();

	let el;
	let u;

	function cssVar(name) {
		return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
	}

	onMount(() => {
		const accent = cssVar('--accent') || '#ffffff';
		const dim = cssVar('--text-dim') || '#9a9aa1';
		const faint = cssVar('--text-faint') || '#5c5c63';
		const grid = 'rgba(255,255,255,0.07)';

		u = new uPlot(
			{
				width: el.clientWidth,
				height: 240,
				padding: [12, 10, 0, 0],
				legend: { show: false },
				cursor: { points: { show: false } },
				scales: { y: { range: [0, 100] } },
				axes: [
					{
						stroke: faint,
						font: '10px "Fira Mono", monospace',
						ticks: { show: false },
						grid: { show: false }
					},
					{
						stroke: faint,
						font: '10px "Fira Mono", monospace',
						size: 42,
						ticks: { show: false },
						grid: { stroke: grid, width: 1, dash: [2, 4] },
						values: (_u, vals) => vals.map((v) => `${v} %`)
					}
				],
				series: [
					{},
					{ label: 'CPU', stroke: accent, width: 1.6, fill: 'rgba(255,255,255,0.055)' },
					{ label: 'RAM', stroke: dim, width: 1.2 }
				]
			},
			[ts, cpu, mem],
			el
		);

		const ro = new ResizeObserver(() => {
			if (el && u) u.setSize({ width: el.clientWidth, height: 240 });
		});
		ro.observe(el);
		return () => {
			ro.disconnect();
			u?.destroy();
		};
	});

	$effect(() => {
		if (u) u.setData([ts, cpu, mem]);
	});
</script>

<div bind:this={el} class="chart"></div>

<style>
	.chart {
		width: 100%;
	}
	/* uPlot kreslí do canvasu — jen ať nedědí user-select kurzory */
	.chart :global(.u-over) {
		cursor: crosshair;
	}
</style>
