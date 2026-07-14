# Kompatibilita, vývoj a distribuce

> Třetí doprovodný dokument k `SPEC.md` a `ROADMAP.md`.
> Řeší: (1) běh na libovolné verzi Windows 10/11, (2) vývoj z macOS na Windows cíl,
> (3) distribuci aktualizací aplikace.
>
> **Rozhodnutí o Nixu: NE.** Zdůvodnění v kap. 3. Toolchain se zamyká nativně.

---

## 1. Kompatibilita napříč Windows 10 a 11

### 1.1 Princip: testuj schopnost, ne verzi

Nástroj stojí na nedokumentovaných a verzně proměnlivých API (`NtQuerySystemInformation`,
`NtSuspendProcess`, ETW schémata). Struktura se **mezi buildy Windows může měnit**. Proto:

- **Žádné napevno zadrátované offsety ani velikosti struktur.** Nikdy nepředpokládej layout.
- **Capability probing místo dotazu na verzi.** Neptej se „je to Win11?", ale „vrací tenhle
  ETW provider tohle pole / je tahle funkce v ntdll exportovaná?". Otestuj schopnost.
- Verzi zjišťuj přes `RtlGetVersion` (z ntdll), **NE `GetVersionEx`** — ta kvůli manifestům
  a kompatibilním shim vrací nepravdivé hodnoty.

### 1.2 Minimální podporovaná hranice

**Windows 10 verze 1809 (build 17763)** a výše, x64.

Pod touhle hranicí chybí části ETW schémat a moderní API a náklady na podporu převýší přínos.
17763+ pokrývá reálně všechny živé instalace Windows 10 i 11.

### 1.3 Konkrétní obranné techniky podle API

| API | Riziko | Obrana |
|---|---|---|
| `NtQuerySystemInformation` | struktura se mění mezi buildy | vždy ověř vrácenou délku; fallback `EnumProcesses` + `GetProcessTimes` při neshodě |
| ETW providery | verze schématu eventu se liší | parsuj podle `EventDescriptor.Version`, ne napevno; drž parsery pro známé verze |
| `NtSuspendProcess` | nedokumentované, nemusí být | fallback: iterace vláken + `SuspendThread`, vždy s párovým resume |
| `PROCESS_POWER_THROTTLING` | jen novější buildy | feature-flag: použij, kde je; jinak přeskoč (není kritické) |
| Restart Manager | stabilní od Visty | bez obav, ale ověř návratové kódy |
| WUA (`IUpdateSearcher`) | chování se liší dle Windows Update konfigurace | ošetři `WU_E_*` chyby, timeout, offline stav |

### 1.4 Feature-flag matice

Jedna centrální tabulka `capabilities` sestavená **jednou při startu služby**:

```rust
// win-sys/src/caps.rs — zjištěno jednou, sdíleno read-only
pub struct Caps {
    pub build: u32,                 // z RtlGetVersion
    pub has_power_throttling: bool, // probe: existuje API?
    pub etw_process_v: u8,          // verze schématu Kernel-Process eventu
    pub has_nt_suspend: bool,       // probe: NtSuspendProcess exportováno?
    // …
}
```

Každý modul se ptá `Caps`, nikdy nedělá vlastní detekci verze. Degradace je vždy **elegantní**:
funkce, která nejde na starším buildu, se v UI zobrazí jako nedostupná s vysvětlením, ne jako pád.

### 1.5 Testovací matice

Minimálně tři cílové buildy ve VM (viz kap. 2.2):
- **Windows 10 22H2** (build 19045) — nejrozšířenější Win10.
- **Windows 11 23H2** (build 22631) — mainstream Win11.
- **Windows 11 24H2** (build 26100) — nejnovější, kde se nejdřív projeví změny API.

Volitelně **Windows 10 1809** jako spodní hranice pro ověření degradace.

Každou verzi (v0–v11) otestuj alespoň na jednom Win10 a jednom Win11 buildu, než zavřeš bránu.

---

## 2. Vývojové prostředí (vyvíjíte na macOS, cíl je Windows)

### 2.1 Co jde dělat na Macu a co ne

| Činnost | Na Macu | Kde doopravdy |
|---|---|---|
| Psaní kódu, `cargo check`, `cargo clippy` | ✓ (kontrola syntaxe i bez Win targetu) | Mac |
| `cargo build --target x86_64-pc-windows-msvc` | ✗ (chybí MSVC linker) | Windows / CI |
| Spuštění služby, ETW, NtQuery* | ✗ (to je Windows kernel) | Windows VM |
| Ladění, testování funkcí | ✗ | Windows VM |

Na Macu tedy **píšete a staticky ověřujete**, běh a testy jsou na Windows VM. `cargo check`
na Macu je rychlá zpětná vazba, že kód aspoň kompiluje, než ho pošlete na build.

> Tip: `rust-analyzer` v editoru nastav na `--target x86_64-pc-windows-msvc`, aby ti
> podtrhával platformně specifické chyby už na Macu.

### 2.2 Windows VM — vaše testovací pole

Na Macu (Apple Silicon) potřebuješ **x64 Windows**, protože nástroj je x64-only a ARM Windows
by emuloval a zkresloval měření výkonu. Možnosti:

- **UTM / QEMU** s x64 Windows 11 — funguje na Apple Siliconu, ale x64 emulace je pomalá.
  Pro funkční testy stačí, pro měření výkonu ne.
- **Cloudový Windows** (nejpraktičtější pro měření): Azure/AWS Windows VM, nebo levněji
  vyhrazený Windows stroj. Reálný výkon, reálné měření rozpočtu.
- **Fyzický Windows PC** (ideál pro finální měření a v7/v8 testy mazání/killu): tvůj domácí
  Windows stroj. Riskantní akce (v7, v8) ale i tak nejdřív ve VM snapshotu, ne na bare metalu.

**Snapshoty jsou nutnost.** Před každým testem v6–v8 udělej snapshot VM. Když test rozbije
systém (a v8 to umí), rollback na snapshot je otázka vteřin. Tohle je hlavní důvod, proč
testovat mutace primárně ve VM, ne na fyzickém stroji.

### 2.3 CI: GitHub Actions na Windows runneru

Skutečný build a testy běží v CI, aby byly reprodukovatelné a nezávislé na tvém stroji.

```yaml
# .github/workflows/build.yml (kostra)
on: [push, workflow_dispatch]
jobs:
  build:
    runs-on: windows-latest        # Windows runner, ne tvůj Mac
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.XX.0         # zamčeno, viz kap. 3
          targets: x86_64-pc-windows-msvc
      - run: cargo build --locked --release --target x86_64-pc-windows-msvc
      - run: cargo test  --locked --target x86_64-pc-windows-msvc
      # Invarianta: validační vrstva musí zůstat izolovaná (SPEC kap. 17.1).
      # Selže, kdyby validate/ začal záviset na exekutorech nebo kolektorech.
      - run: cargo tree -p validate --edges normal | Select-String "actor-|collector-|store|ipc|ui" && exit 1 || exit 0
      # podpis + balení + publikace artefaktu → viz kap. 4
```

`--locked` vynutí přesně verze z `Cargo.lock` — build je bit-reprodukovatelný napříč stroji.
`windows-latest` runner je zdarma pro veřejné repo, u privátního v rámci měsíční kvóty.
Poslední krok je **strážce nejdůležitější architektonické invarianty** — izolace validační vrstvy (SPEC kap. 2.2, 17.1). Když ji někdo poruší, build spadne.

---

## 3. Verzování toolchainu — nativně, bez Nixu

### 3.1 Rozhodnutí a proč

**Nix se nepoužije.** Reprodukovatelnost, kterou od něj chceš, dostaneš nativně, a Nix by
na Windows targetu byl čistá zátěž bez výnosu:

| Co chceš | Nix na Windows | Nativní řešení |
|---|---|---|
| Zamčená verze Rust toolchainu | ✗ (funguje jen ve WSL, kde ale nestavíš Windows binárku) | `rust-toolchain.toml` ✓ |
| Zamčené verze crate | to řeší Cargo, ne Nix | `Cargo.lock` ✓ |
| Reprodukovatelný build | ✗ na `x86_64-pc-windows-msvc` | `cargo build --locked` v CI ✓ |
| Nativní PE binárka volající ntdll | ✗ (Nix neumí produkovat MSVC PE) | MSVC toolchain ✓ |
| Podepsaná MSI | ✗ | WiX + signtool ✓ |

Nix neprodukuje nativní Windows binárky. Nástroj *je* Windows služba volající `ntdll` a ETW —
musí být zkompilovaná MSVC toolchainem do PE a podepsaná. Cross-kompilace přes MinGW je přesně
prostředí, kde nedokumentovaná NT API a `windows-rs` linkování začnou zlobit.

### 3.2 Co se zamyká místo Nixu

```toml
# rust-toolchain.toml — v kořeni repo, zamyká toolchain pro všechny
[toolchain]
channel = "1.XX.0"                        # konkrétní verze, ne "stable"
targets = ["x86_64-pc-windows-msvc"]
components = ["rustfmt", "clippy"]
```

- `Cargo.lock` **commitnutý** → zamčené přesné verze všech závislostí.
- `cargo build --locked` v CI → build selže, když by `Cargo.lock` neseděl. Žádný drift.
- Verze WiX, signtool a dalších nástrojů zafixované v CI workflow (konkrétní verze actions).

Tím máš plnou reprodukovatelnost buildu, nativně, s funkčním podpisem na konci řetězce.

---

## 4. Distribuce a aktualizace aplikace

### 4.1 Dva kanály

- **Dev kanál** — artefakt z **každého commitu** (nebo z push na `main`). Sem míří tvoje
  vývojová VM. Rychlá smyčka: commit → za pár minut je build → VM se sama aktualizuje.
- **Stable kanál** — jen z **git tagů** (`v0.1.0`, `v0.2.0`…). Sem půjdou reálná vydání.
  Nikdy ne z každého commitu.

Kanál je jen jiná cesta k manifestu (viz níže) — stejný mechanismus, jiný feed.

### 4.2 Tauri v2 built-in updater

Tauri v2 má vestavěný aktualizační plugin — použij ho, nevymýšlej vlastní. Funguje takto:

1. Aplikace si stáhne **`latest.json`** manifest z tvého endpointu (GitHub Releases stačí).
2. Porovná verzi a **ověří podpis** manifestu (Tauri má vlastní podpisový klíč, oddělený od
   code signing certifikátu — brání podvržení updatu).
3. Stáhne balíček, ověří, nainstaluje, restartuje.

```json
// latest.json — publikováno CI do GitHub Releases
{
  "version": "0.2.0",
  "notes": "…",
  "pub_date": "2026-…",
  "platforms": {
    "windows-x86_64": {
      "signature": "…",                    // Tauri updater podpis
      "url": "https://github.com/…/syswatch_0.2.0_x64.msi"
    }
  }
}
```

### 4.3 Háček: aktualizace SLUŽBY, ne jen UI

Tauri updater umí aktualizovat **aplikaci**. Ale tvoje architektura má dvě části:
UI (běžný proces) **a službu** (běží jako SYSTEM). To updater sám neřeší. Postup:

1. Updater stáhne nový MSI a spustí ho (elevace přes UAC — nutné pro zápis do služby).
2. MSI (WiX) provede: `sc stop syswatch` → přepíše binárku služby → `sc start syswatch` →
   aktualizuje UI.
3. **Verzní kontrakt IPC.** UI a služba si při připojení vymění verzi protokolu. Když se
   liší (updatovaná jen jedna část), UI zobrazí „čekám na dokončení aktualizace", nespadne.
   Protokol drž zpětně kompatibilní, nebo verzuj zprávy.

Tohle je jediné netriviální místo distribuce — naplánuj ho, ať tě nepřekvapí u v11.

### 4.4 Podpis — dvě různé věci, neplést

| Podpis | K čemu | Kdy |
|---|---|---|
| **Code signing** (Authenticode, OV/EV certifikát) | aby Windows/SmartScreen/antivir binárce věřil | na MSI i EXE, v CI |
| **Updater signing** (Tauri klíč) | aby nešel podvrhnout `latest.json`/balíček | na manifest, v CI |

Code signing certifikát je placený (OV řádově tisíce Kč/rok, EV dráž ale s okamžitou
SmartScreen reputací). Pro vývoj a testování ve VM ho nepotřebuješ — self-signed nebo
nepodepsané stačí, jen odklikáš varování. **Ostrému vydání se ale bez code signingu
nevyhneš** — nepodepsaná systémová služba je pro ostatní uživatele varovný signál a část
antivirů ji rovnou zablokuje.

### 4.5 Balení: WiX (MSI), ne NSIS

Pro nástroj, který registruje **Windows službu**, je MSI přes WiX správná volba:
- deklarativní instalace/odebrání služby (`ServiceInstall`/`ServiceControl`),
- korektní odinstalace (nezůstane viset služba),
- Tauri v2 umí MSI (WiX) jako bundle target.

NSIS je jednodušší, ale instalaci služby a rollback řeší hůř. U systémového nástroje jdi do MSI.

---

## 5. Doporučené pořadí zavedení infrastruktury

Tohle se **prolíná** s roadmapou z `ROADMAP.md` — nedělá se najednou, ale postupně:

| Kdy (dle ROADMAP) | Co z tohoto dokumentu zavést |
|---|---|
| **před v0** | repo, `rust-toolchain.toml`, `Cargo.lock`, základní CI (`cargo check` + build na `windows-latest`) |
| **v0** | Windows VM se snapshotem, ruční kopírování buildu z CI do VM |
| **v1** | `Caps` capability probing (kap. 1.4) — hned, ať na něm staví všechny kolektory |
| **v2–v4** | testovací matice: každou bránu ověř na Win10 i Win11 VM |
| **v5+** | povinné VM snapshoty před každým testem mutací |
| **v11** | Tauri updater, dva kanály, WiX MSI, code signing, aktualizace služby |

Auto-updater a podpis necháváš záměrně na v11 — do té doby stačí ručně kopírovat build z CI
do VM. Předčasně stavět distribuční pipeline by tě zdrželo od funkcí. Ale **CI a capability
probing zaveď hned na začátku** — ty ti šetří čas každý den.
