// Kódy problémů zařízení (CM_PROB_*) přeložené do lidské řeči.
//
// Správce zařízení ukazuje jen číslo a suchou větu. Uživatel z toho
// nepozná, jestli má něco dělat. Proto ke každému kódu patří dvojice:
// co se děje a co to pro něj znamená — bez strašení a bez rad, které
// by mohly rozbít víc, než opraví.
//
// Kódy podle dokumentace Configuration Manageru; pokrývá se to, co se
// reálně objevuje. Neznámý kód se nepředstírá — řekne se, že ho neznáme.

const PROBLEMS = {
	1: {
		what: 'zařízení není správně nastavené',
		means: 'Windows k němu mají neúplné informace. Obvykle pomůže přeinstalovat ovladač.'
	},
	3: {
		what: 'ovladač je poškozený, nebo došla paměť',
		means: 'Zařízení se nedá spustit s ovladačem, který je nainstalovaný.'
	},
	9: {
		what: 'systém zařízení nerozpozná',
		means:
			'Informace o něm v registru jsou neúplné nebo poškozené. Bývá to zbytek po programu, který se neodinstaloval celý — zařízení pak fyzicky neexistuje a jen straší v seznamu.'
	},
	10: {
		what: 'zařízení se nepodařilo spustit',
		means: 'Hardware je vidět, ale nenaběhl. Někdy stačí odpojit a znovu připojit.'
	},
	12: {
		what: 'nedostatek volných prostředků',
		means: 'Dvě zařízení si nárokují totéž (přerušení, adresní rozsah) a jedno musí ustoupit.'
	},
	14: {
		what: 'čeká na restart',
		means: 'Zařízení začne fungovat po restartu počítače.'
	},
	16: {
		what: 'Windows neznají všechny prostředky, které zařízení používá',
		means: 'Zařízení je starší typ, který se sám nehlásí. Prostředky se nastavují ručně.'
	},
	18: {
		what: 'ovladače je potřeba přeinstalovat',
		means: 'Instalace ovladače neproběhla do konce.'
	},
	19: {
		what: 'poškozený záznam v registru',
		means: 'Konfigurace zařízení je rozbitá — Windows ji neumí přečíst.'
	},
	21: {
		what: 'systém zařízení právě odebírá',
		means: 'Přechodný stav. Za chvíli zmizí ze seznamu samo.'
	},
	22: {
		what: 'zařízení je zakázané',
		means: 'Někdo ho vypnul ve Správci zařízení. Nejde o poruchu.'
	},
	24: {
		what: 'zařízení není přítomné nebo nefunguje',
		means: 'Bývá odpojené, nebo je jeho instalace neúplná.'
	},
	28: {
		what: 'ovladače nejsou nainstalované',
		means: 'Windows pro tenhle hardware nenašly ovladač.'
	},
	29: {
		what: 'firmware zařízení mu nepřidělil prostředky',
		means: 'Zařízení bývá vypnuté v BIOSu/UEFI.'
	},
	31: {
		what: 'Windows nemohou načíst ovladače',
		means: 'Ovladač je nekompatibilní nebo ho blokuje systém.'
	},
	43: {
		what: 'zařízení hlásí poruchu a systém ho zastavil',
		means: 'Ohlásil ji sám hardware nebo jeho ovladač.'
	},
	45: {
		what: 'zařízení není připojené',
		means: 'Windows si ho pamatují z minula. Jakmile ho připojíš, ožije.'
	}
};

/// Popis problému, nebo `null` když zařízení běží v pořádku.
export function describeProblem(code) {
	if (!code) return null;
	return (
		PROBLEMS[code] ?? {
			what: `systém hlásí problém ${code}`,
			means: 'Tenhle kód neznáme — detail najdeš ve Správci zařízení.'
		}
	);
}
