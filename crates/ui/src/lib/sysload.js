// Celková zátěž systému z dílčích metrik.
//
// Průměr sám o sobě lže: stroj, který má procesor na stu a zbytek na
// nule, není zatížený na třicet procent — je zaseknutý. Čistý průměr
// by to schoval, čisté maximum by zas hlásilo poplach pokaždé, když
// se na chvilku rozběhne jedno jádro. Proto se průměr s rostoucím
// maximem přiklání k maximu.
//
// Bydlí to tady, protože z toho počítá graf v Tasks i dlaždice na
// Home. Dvě kopie by se rozešly a totéž číslo by na dvou místech
// vyšlo jinak.

/// `slozky` jsou procenta; `null` a NaN se přeskakují (metriku, kterou
/// stroj nehlásí, nemá cenu započítávat jako nulu).
export function zatezSystemu(slozky) {
	const vals = slozky.filter((v) => v != null && !Number.isNaN(v));
	if (!vals.length) return 0;
	const mean = vals.reduce((a, b) => a + b, 0) / vals.length;
	const max = Math.max(...vals);
	const w = Math.min(max / 100, 1);
	return mean * (1 - w) + max * w;
}
