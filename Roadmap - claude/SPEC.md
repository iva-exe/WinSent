# Systémový monitor pro Windows — technická specifikace

> Handoff dokument pro implementaci. Cílová platforma: **Windows 10 build 17763+ (1809) i Windows 11, x64**.
> Stack: **Rust** (jádro, služba), **Tauri v2 + SvelteKit** (UI), **SQLite** (úložiště).
> Paradigma: **funkcionální / procedurální**. Žádné třídy, žádná dědičnost. Data + volné funkce.
>
> Doprovodné dokumenty: `ROADMAP.md` (pořadí stavby, jediný zdroj pravdy pro fáze), `INFRA.md` (kompatibilita, vývoj, distribuce).

---

## 1. Co aplikace dělá

Rezidentní systémový nástroj — **informační švýcarský nůž pro Windows**. Nepřetržitě, levně a bezpečně sleduje stav PC a odpovídá na otázky, na které vestavěné nástroje neodpovídají dobře.

**Naprostá většina funkcí jen čte.** Všude, kde se mění stav systému — odinstalace (Apps), mazání (Files), startup přepínače (Start), přepínače soukromí (Security), opt-in aktualizace ovladače (Drivers) — vede **jediná cesta: validační vrstva** (kap. 17). Žádný modul nemá vlastní obchvat.

Navigace UI = sekce nástroje, rozdělené podle povahy:

| Skupina | Sekce | Otázka | Povaha |
|---|---|---|---|
| **Stav** | Home | Rychlý přehled celého PC (grid dlaždic) | čtení |
| | Tasks | Co běží, per aplikace, + historie a záseky | čtení |
| | Hardware | Jaký hardware mám a v jakém je stavu | čtení |
| | Network | Co komunikuje ven, kam, kudy, kolik | čtení |
| | Security | Jsem chráněný a soukromý? Kdo má jaká oprávnění a kdo je právě používá? | čtení + vratné přepínače |
| | Users | Kdo má na PC účet a jaká práva | čtení |
| **Správa** | Apps | Co mám nainstalované, kde to žije, odinstalace | čtení + **mutace** |
| | Files | Disky, oddíly, prohlížeč, mazání se závislostmi | čtení + **mutace** |
| | Start | Co startuje s Windows, zapnout/vypnout | **vratná mutace** |
| | Drivers | Ovladače a verze, opt-in aktualizace | čtení + opt-in mutace |
| — | Settings | Konfigurace nástroje | — |

**Nefunkční požadavky (tvrdé):**
- Kolektor: **< 0,5 % CPU**, **< 50 MB RAM**, **< 250 MB/den** na disk po retenci.
- Kolektor běží 24/7, přežívá zamrznutí desktopu a zaznamená ho.
- **Stabilita nad vším.** Aplikace nesmí nikdy destabilizovat systém. Každá mutující akce je vratná nebo checkpointovaná. Když si nejsme jistí, akci neprovedeme. Safe > sorry je tvrdé pravidlo, ne preference.
- **Kompatibilita Windows 10 (build 17763+) i 11, x64.** Žádné napevno zadrátované offsety struktur. Capability probing místo dotazu na verzi. Detaily v `INFRA.md` kap. 1.
- **Každá mutující akce prochází nezávislou validací** (kap. 17) — plán → validace → provedení → ověření.
- **Levné čtení jako princip.** Nová sekce si zaslouží slot, jen když: (1) je čtecí nebo plně vratná, (2) odpovídá na otázku, kterou Windows neřeší dobře, (3) vejde se do rozpočtu bez vlastního kernel driveru.
- **Sebetransparentnost.** Nástroj se nikdy neskrývá před vlastními výpisy — své procesy, soubory, spotřebu i startup položku ukazuje jako u kterékoli jiné aplikace. Skrývání se je chování malwaru. Rozpočet výkonu musí být pro uživatele **ověřitelný, ne slibovaný** (kap. 2.3).
- **Nikdy nepředstírat bezpečnostní záruku, kterou nemáme.** Kde Windows něco nevynucuje (typicky oprávnění u Win32 aplikací), musí to UI přiznat. Falešný pocit ochrany je horší než žádný (kap. 13.4).
- Vše modulární — každý kolektor je samostatná crate, kterou lze vypnout.

**Mimo rozsah (explicitně):**
- Nix/deklarativní správa balíků. Neimplementovat.
- **Nouzový nativní panel.** UI je pouze Tauri. Přijímáme, že při tvrdém záseku systému UI nemusí vykreslit. Garanci dostupnosti drží **démon**, ne UI — data z kritické vteřiny existují a uživatel se k nim vrátí, jakmile se systém probere.
- **Vlastní stahování ovladačů z webu výrobců.** Viz kap. 6.2 — bezpečnostní riziko, defer na Windows Update / winget.
- **Násilné zavírání cizích handle** (`DUPLICATE_CLOSE_SOURCE` na cizí proces). Destabilizuje vlastníka. Viz kap. 18.3 — zakázaný vzor.
- **Force delete souborů aplikace bez uninstalleru.** Nahrazeno asistovaným vratným odstraněním. Viz kap. 5.3.
- **Vlastní antivirus / skener malwaru.** Defender to umí. Security jen čte stav a ukazuje signály, nedává verdikty. Viz kap. 13.
- **Čištění registru a „optimalizační tweaky"** (vypínání služeb, prefetch). Žádný měřitelný přínos, hlavní zdroj rozbití systému. Neimplementovat.
- **Full-text index obsahu disku.** Rozbíjí rozpočet. Vyhledávání staví na NTFS MFT/USN. Viz kap. 11.
- **Hluboká inspekce obsahu paketů (DPI).** Vyžaduje kernel driver a u TLS je obsah stejně šifrovaný. Network ukazuje cíl a objem, ne obsah. Viz kap. 12.
- **Vlastní kernel driver pro senzory** (WinRing0 apod.). V blocklistu, popírá stabilitu. GPU teploty jdou přes NVML/ADLX/IGCL, CPU přes degradační kaskádu (kap. 15.2) — bez vlastního driveru.
- **Percepční „podobné soubory".** Jen bit-identické duplicity, on-demand. Viz kap. 11.3.
- **Vlastní hodnocení, že jmenovaná aplikace „sbírá data".** Jen fakta, která nástroj sám změří (oprávnění, pozorované endpointy). Viz kap. 13.3.
- **Předstírání macOS-like tvrdého zámku oprávnění.** Windows u Win32 aplikací oprávnění tvrdě nevynucuje. Zobrazujeme viditelnost a upřímné rozlišení vynuceno/nevynuceno, ne falešnou jistotu. Viz kap. 13.4.
- **Anti-tamper obrana proti adminovi** (bránit se ukončení, blokovat odinstalaci). To je chování malwaru. Když admin službu zastaví, má na to právo — UI to jen ukáže. Viz kap. 2.3.

---

## 2. Architektura

```
┌──────────────────────── Zdroje dat ────────────────────────┐
│ ETW session │ NtQuerySystemInfo │ Inventář │ Senzory │ MFT  │
│ (události)  │ (vzorky 1 Hz)     │(on-demand)│ (1 Hz) │(USN) │
└──────┬──────┴─────────┬─────────┴─────┬─────┴────┬────┴──┬──┘
       │                │               │          │       │
┌──────▼────────────────▼───────────────▼──────────▼───────▼──┐
│  DÉMON — Windows Service, LocalSystem, session 0            │
│                                                              │
│  ČTECÍ CESTA                     MUTUJÍCÍ CESTA              │
│  ┌──────────────┐  ┌─────────┐   ┌────────────────────────┐ │
│  │ Ring buffer  │─▶│ Flusher │   │  validate/  (kap. 17)  │ │
│  │ RAM, locked  │  │ dávkově │   │  ── samostatná ──      │ │
│  └──────────────┘  └────┬────┘   │  registry akcí         │ │
│                         │        │  plán→valid→exec→ověř  │ │
│                         │        └───────────┬────────────┘ │
│                         │                    │ volá         │
│                         │             ┌──────▼───────┐      │
│                         │             │  actor-*     │      │
│                         │             │ (exekutory)  │      │
│                         │             └──────────────┘      │
└─────────────────────────┼───────────────────────────────────┘
                          ▼
               ┌─────────────────────┐
               │  SQLite (WAL)       │
               │  retence + audit    │
               └──────────┬──────────┘
                          │ named pipe (IPC)
               ┌──────────▼──────────┐
               │ Tauri v2 + SvelteKit│
               │ UI, běžný uživatel  │
               └─────────────────────┘
```

**Dvě oddělené cesty.** Čtecí (sběr → ring → SQLite) je horká, běží 24/7, musí být levná. Mutující (validate → actor) je studená, běží jen na vyžádání, musí být **neprůstřelná**. Nesdílí kód ani stav — kolektor nemůže omylem něco změnit a validátor nemůže zpomalit sběr.

### 2.1 Oddělení privilegií

- **Služba** běží jako `LocalSystem`, session 0. Má `SeDebugPrivilege`, `SeSystemProfilePrivilege` (nutné pro ETW kernel providery).
- **UI** běží jako běžný uživatel. **Nikdy jako admin.** UI je čistě zobrazovací.
- Každá destruktivní akce (kill, disable startup, uninstall) jde jako **požadavek přes named pipe** a služba ji validuje proti allowlistu. UI si nesmí nic vynutit.
- Named pipe: `\\.\pipe\syswatch` s DACL omezenou na členy skupiny `Users` interaktivní session.

### 2.2 Rozdělení do crates (Cargo workspace)

```
/crates
  core-types/      # sdílené datové typy, žádné závislosti na Windows
  win-sys/         # tenké safe wrappery nad windows-rs (NtQuery*, SetupAPI, WUA, Task Scheduler) + Caps (INFRA kap. 1.4)

  # ── ČTECÍ CESTA (horká, 24/7, levná) ──
  identity/        # rozpoznání aplikace z procesu/cesty + cache podpisů + override
  collector-proc/  # sampler procesů (1 Hz)
  collector-etw/   # ETW session: ProcessStart/Stop, FileIo, DiskIo, hard faults, Network, DXGI
  collector-inv/   # inventář aplikací (registry, MSI, MSIX) + mapa souborů
  collector-drv/   # ovladače (SetupAPI) + kontrola aktualizací (WUA)
  collector-boot/  # startup položky — ČTENÍ (6 backendů)
  collector-net/   # spojení per PID, porty, trafik, WiFi (kap. 12)
  collector-sec/   # stav ochrany, signály procesů, telemetrie, OPRÁVNĚNÍ (ConsentStore) — ČTENÍ (kap. 13)
  collector-users/ # účty, oprávnění, historie přihlášení (kap. 14)
  collector-hw/    # inventář hardwaru + SMART + baterie (kap. 15.1)
  collector-sensors/ # GPU teploty (NVML/ADLX/IGCL), CPU kaskáda, FPS/frame time (kap. 15.2–15.3)
  collector-crash/ # incidenty: pády, hangy, BSOD, parsování minidumpů (kap. 16)
  collector-lock/  # Restart Manager + handle scan (identifikace držitelů, kap. 18.1)
  fs-index/        # NTFS MFT/USN čtení, vyhledávání, duplicity (kap. 11)

  # ── MUTUJÍCÍ CESTA (studená, na vyžádání, neprůstřelná) ──
  validate/        # ⚠ VALIDAČNÍ VRSTVA (kap. 17) — samostatná, závisí JEN na core-types + win-sys
  actor-toggle/    # rychlé přepínače: startup, soukromí, oprávnění (ConsentStore), driver opt-in (T0)
  actor-file/      # bezpečné mazání, suspend/resume, delay-until-reboot (kap. 18)
  actor-app/       # odinstalace + úklid zbytků + asistované odstranění (kap. 5.3–5.4)
  actor-proc/      # ukončování procesů + trasování závislostí (kap. 18.4)

  # ── INFRASTRUKTURA ──
  store/           # SQLite schéma, zápis, retence, audit log, dotazy
  ipc/             # protokol named pipe (postcard, délkově prefixované rámce)
  svc/             # host Windows služby (binárka)
  ui/              # Tauri v2 + SvelteKit (binárka)
```

**Tvrdé pravidlo závislostí — validační vrstva je samostatná:**

```
validate/  ──závisí na──▶  core-types, win-sys        ✅ a NIC VÍC
validate/  ──NESMÍ záviset na──▶  actor-*, collector-*, store, ipc, ui   ❌
actor-*    ──závisí na──▶  validate/, win-sys, core-types
```

Důvod: validátor musí jít **zkompilovat, otestovat a spustit úplně sám**, bez zbytku aplikace. Kdyby záležel na exekutoru nebo na kolektorech, ztratil by nezávislost (validoval by proti témuž kódu, který má hlídat) a nešel by testovat izolovaně. Tuhle hranici v CI hlídej — `cargo tree -p validate` nesmí obsahovat žádný `actor-*` ani `collector-*`.

Každý `collector-*` implementuje stejný tvar rozhraní (ne trait s dědičností — jen konvence):

```rust
pub fn init(cfg: &Config) -> Result<State>;
pub fn tick(state: &mut State, out: &mut RingWriter) -> Result<()>;
pub fn shutdown(state: State);
```

### 2.3 Sebemonitoring a sebeobrana

Nástroj hlídá i sám sebe. Není to jen konzistence — je to **bezpečnostní a důvěryhodnostní funkce**.

**Tvrdý zákaz sebeskrývání.** Aplikace se **nikdy** nevyloučí z vlastních výpisů:
- Její procesy jsou v Tasks jako každé jiné, se skutečnou spotřebou.
- Její soubory jsou v Apps a Files jako každé jiné.
- Její startup položka je v Start **viditelná a vypnutelná** (jen s varováním, viz níže).
- Její vlastní mutace jsou v auditu (kap. 17.6).

Skrývání se před vlastními výpisy je definiční chování malwaru. Naopak — **transparentnost je tady featura**: uživatel si má ověřit, že démon opravdu žere 0,3 % CPU. Proto v Settings existuje dlaždice **„Spotřeba nástroje"**: živé CPU/RAM, zapsáno dnes/celkem, velikost DB. Rozpočet z kap. 20 je tak pro uživatele **ověřitelný, ne slibovaný**.

**Vlastní zátěž se přiznává, nemaskuje.** Když flush do SQLite způsobí I/O špičku, objeví se v atribuci záseku jako kterýkoli jiný proces — jen označená `[self]`. Kdyby si nástroj vlastní zátěž odfiltroval, zatajil by uživateli reálnou příčinu problému.

**Watchdog — zadarmo, přes OS.** Service Recovery restartuje službu po pádu bez vlastního kódu:
```
sc failure syswatch reset=86400 actions=restart/5000/restart/10000/restart/30000
```
Plus kontrola integrity: při startu služba ověří Authenticode podpis vlastních binárek. Neshoda → **nespustí se a nahlásí to**, místo aby běžela podvržená.

**Žádná anti-tamper obrana proti adminovi.** Bránit se uživateli s admin právy (chránit se před ukončením, blokovat odinstalaci) je opět chování malwaru. Když admin službu zastaví, má na to právo. UI to jen ukáže červeným indikátorem v hlavičce (kap. 9.2) — uživatel ví, že monitoring neběží.

**Vlastní startup položka.** V sekci Start se nástroj zobrazí jako každý jiný, ale s varováním: *„Vypnutím zastavíte monitorování — historie a incidenty se přestanou zaznamenávat."* Zůstává vypnutelný. Není skrytý, není zamčený.

**Pozor na zpětnou vazbu:** sampler měří i sám sebe. To je správně, ale nesmí vzniknout smyčka (měření → zátěž → větší měření). Vlastní vzorek se počítá stejně jako ostatní, žádný speciální kód navíc.

---

## 3. Sběr dat — konkrétní API

### 3.1 Vzorkování procesů (1 Hz)

**Použij `NtQuerySystemInformation(SystemProcessInformation)`.** Jedno volání vrátí všechny procesy i vlákna v jednom bufferu (~300 µs).

**NEPOUŽÍVEJ** `EnumProcesses` + `OpenProcess` v cyklu — to je řádově dražší a selhává na chráněných procesech.

Z `SYSTEM_PROCESS_INFORMATION` čti:
- `UniqueProcessId`, `InheritedFromUniqueProcessId`
- `KernelTime`, `UserTime` (delta oproti minulému vzorku → CPU %)
- `WorkingSetPrivateSize`, `PrivatePageCount`, `WorkingSetSize`
- `PageFaultCount`, `HardFaultCount` ← **kritické pro detekci záseků**
- `ReadTransferCount`, `WriteTransferCount`, `OtherTransferCount`
- `ImageName`, `SessionId`

Buffer alokuj jednou, realokuj jen při `STATUS_INFO_LENGTH_MISMATCH`. **V hot pathu žádné alokace.**

Systémové metriky (`system_sample`): total CPU, commit charge, disk queue length, hard fault rate, thermal throttling. GPU přes `D3DKMTQueryStatistics` nebo PDH counter `\GPU Engine(*)\Utilization Percentage` (volitelně, je to drahé — default vypnuto).

### 3.2 ETW session

Vytvoř **jednu** ETW session (`StartTrace` / `EnableTraceEx2`), realtime mód.

Providery a proč:

| Provider | Účel | Objem |
|---|---|---|
| `Microsoft-Windows-Kernel-Process` | ProcessStart/Stop, ImageLoad, exit codes (pády) | nízký |
| `Microsoft-Windows-Kernel-File` (filtrovaný) | mapa souborů aplikací | **vysoký — filtruj!** |
| `Microsoft-Windows-Kernel-Disk` | I/O latence při zámrzu | střední |
| `Microsoft-Windows-Kernel-Memory` (hard faults) | swap/paging při zámrzu | nízký |
| `Microsoft-Windows-Kernel-Network` | trafik per PID (kap. 12) | střední — filtruj |
| `Microsoft-Windows-DXGI` + `Microsoft-Windows-Dwm-Core` | FPS / frame time (kap. 15.3) | **cíleně, jen kreslící procesy** |

**Klíčová výhoda ETW:** `ProcessStart` nese **skutečný parent PID v okamžiku vzniku**. Tím padá celý problém s recyklací PID — strom stavíš z časové osy, ne z aktuálního stavu.

**Filtr pro Kernel-File** (jinak stovky MB/min):
- Ignoruj: `\Temp\`, `\Windows\Temp\`, `\INetCache\`, `\Cache\`, `*.tmp`, `*.log`, `\Windows\Prefetch\`
- Ignoruj: procesy `System`, `MemCompression`, antivirové skenery
- Zaznamenávej pouze: **první zápis** aplikace do dané cesty (dedup v paměti přes hash set), a to jen `Create`/`Write` operace mimo instalační adresář aplikace.
- Režim „učení mapy souborů" je **volitelný, defaultně vypnutý**, s vlastním přepínačem v UI.

**Filtr pro DXGI/Dwm (FPS):** zapínej jen pro procesy aktivně kreslící přes GPU. Agreguj v kolektoru na sekundové statistiky (počet snímků + min/avg/p95/p99/max frame time), **neukládej jednotlivé snímky** — viz kap. 15.3.

**Černá skříňka — POVINNÁ (ne volitelná).** Paralelní ETW **autologger** session ve file módu (`.etl`). Buffery zapisuje **jádro, ne tvůj proces** → přežije BSOD i hladovění služby. Slouží jako forenzní zdroj pro incidenty (kap. 16.3): SQLite se `synchronous=NORMAL` ztratí při BSODu poslední flush (~2 s), tedy přesně okamžik pádu — autologger tuhle díru zaceluje. Rotující ring 64 MB.

### 3.3 Detekce záseku

V kolektoru běží **heartbeat vlákno** na `THREAD_PRIORITY_TIME_CRITICAL`, tick 100 ms, měří `QueryPerformanceCounter`.

```
if actual_delta > expected_delta * 3:
    zaznamenej stall_event { start, duration }
    přepni sampler na 10 Hz na následujících 10 s
    dumpni z ring bufferu okno T-10s .. T+10s
    klasifikuj příčinu (viz níže)
```

**Klasifikace příčiny záseku** (v tomto pořadí — CPU je nejméně častá příčina):
1. `hard_fault_rate` skok → **paging / nedostatek RAM**
2. `disk_queue_length` > 8 nebo `disk_latency_ms` > 200 → **I/O saturace** (identifikuj proces s největším I/O v okně)
3. `thermal_throttle` flag → **teplotní omezení**
4. `total_cpu` > 95 % → **CPU saturace**
5. Jinak → **neznámé** (typicky driver/DPC — zaznamenej, ale netvrď příčinu)

Do UI se ukazuje: časová osa, viník, a 3–5 top procesů podle příslušné metriky v okně.

### 3.4 Ring buffer

- Preallokovaný, pevná velikost (default 16 MB), lock-free SPSC.
- `VirtualLock` na celý rozsah → nesmí být odswapován.
- `SetProcessWorkingSetSizeEx` s `QUOTA_LIMITS_HARDWS_MIN_ENABLE` → OS netrimuje working set.
- Zápisové vlákno: `HIGH_PRIORITY_CLASS`. **NIKDY `REALTIME_PRIORITY_CLASS`** — ta předbíhá i ovladače a dokáže systém zaseknout sama.
- Flusher (samostatné vlákno, `BELOW_NORMAL`) vyprazdňuje ring do SQLite v dávkách po ~2 s.
- **Sampler nikdy nesmí blokovat na I/O.** Když je zásek způsobený diskem, blokující zápis vás zabije spolu se zbytkem systému.

---

## 4. Identita aplikací — seskupování procesů

Nejdůležitější a nejchytřejší část celé aplikace. Cíl: všechny `chrome.exe --type=renderer` pod jednou položkou **Chrome**; všechny `svchost.exe`, `csrss.exe`, `dwm.exe` pod položkou **Windows**.

### 4.1 Rozhodovací kaskáda

Pro každý nový proces vyhodnoť v tomto pořadí, první shoda vyhrává:

```
0. Override    → uživatelský přepis z tabulky identity_override (viz 4.4)
                identity_key = uložená hodnota. NEJVYŠŠÍ PRIORITA.

1. MSIX/AppX  → PackageFamilyName z GetPackageFamilyName(hProcess)
                identity_key = "msix:{PackageFamilyName}"

2. Windows OS → image_path začíná %SystemRoot% AND Authenticode publisher
                obsahuje "Microsoft Windows"
                identity_key = "os:windows"
                (POZOR: "Microsoft Corporation" ≠ "Microsoft Windows" —
                 Office a Edge musí zůstat samostatné aplikace)

3. Uninstall  → najdi nejdelší InstallLocation z registru Uninstall klíčů,
                který je prefixem image_path
                identity_key = "app:{UninstallKeyName}"

4. Podpis     → Authenticode subject CN + ProductName z VERSIONINFO
                identity_key = "sig:{sha256(CN + ProductName)}"

5. Fallback   → adresář binárky
                identity_key = "path:{parent_dir}"
```

### 4.2 Cache podpisů — jinak vám to sežere CPU

Ověření Authenticode (`WinVerifyTrust`) trvá **jednotky až desítky ms**. Nikdy ho nedělej v samplovacím cyklu.

```rust
// klíč: (image_path, file_size, mtime)  → hodnota: SignatureInfo
// perzistentní tabulka v SQLite, in-memory HashMap v RAM
// lookup: O(1), miss → zařaď do fronty pro background vlákno (BELOW_NORMAL)
```

Rozlišení podpisů:
- **Katalogový podpis** (většina Windows binárek) — ověřuj přes `WTD_CHOICE_CATALOG`, ne `WTD_CHOICE_FILE`, jinak ti systémové soubory vyjdou jako nepodepsané.

### 4.3 Třídy ochrany procesů

```rust
enum ProtectionClass {
    Critical,   // csrss, wininit, services, smss, System, Registry
                // → kill způsobí BSOD (CRITICAL_PROCESS_DIED). ZAKÁZÁNO. Šedě, bez tlačítka.
    Protected,  // PPL procesy (lsass při RunAsPPL, antivirus, MsMpEng)
                // → kill technicky nemožný. Zobraz důvod.
    System,     // ostatní procesy pod SYSTEM/SERVICE
                // → kill možný, ale ZA POTVRZENÍM s varováním
    User,       // procesy uživatele
                // → volně
}
```

Detekce Critical: `NtQueryInformationProcess(ProcessBreakOnTermination)` → pokud `TRUE`, je kritický. Nespoléhej na hardcoded seznam jmen, ale měj ho jako pojistku.

Detekce Protected: `PROCESS_EXTENDED_BASIC_INFORMATION` → `IsProtectedProcess`.

### 4.4 Uživatelský override — „vždy chytré" má strop, tohle je řešení

Kaskáda určí identitu správně v ~99 % případů. Zbytek (portable .exe bez podpisu a bez uninstall záznamu) nemá spolehlivý zdroj pravdy a **nelze ho určit automaticky**. Nepředstírej opak.

Řešení místo dokonalého algoritmu:
1. UI vždy zobrazuje **confidence** identity. `Guess`/`path` fallback je vizuálně odlišený (tečkovaný podtrh).
2. Uživatel může identitu ručně přepsat: *„tenhle proces patří pod aplikaci X"*.
3. Override se uloží a v kaskádě má **nejvyšší prioritu** (krok 0). Systém se tak nikdy neplete dvakrát stejně — to je ta „chytrost".

```sql
CREATE TABLE identity_override (
  match_kind  TEXT NOT NULL,   -- 'exact_path' | 'dir_prefix' | 'sig_subject'
  match_value TEXT NOT NULL,   -- cesta / prefix / subject podle match_kind
  identity_key TEXT NOT NULL,  -- cílová aplikace
  created_ts  INTEGER NOT NULL,
  PRIMARY KEY (match_kind, match_value)
);
```

---

## 5. Inventář aplikací + mapa souborů

### 5.1 Zdroje seznamu aplikací

| Zdroj | API | Přesnost |
|---|---|---|
| Registry Uninstall | `HKLM\SOFTWARE\...\Uninstall`, `HKLM\...\WOW6432Node\...\Uninstall`, `HKCU\...\Uninstall` | vysoká |
| MSI | `MsiEnumProducts` + `MsiGetProductInfo` | vysoká |
| MSIX/AppX | `PackageManager.FindPackages` (WinRT) | úplná |
| winget | `winget list --format json` (volitelně) | doplňkové |

### 5.2 Mapa souborů — kde všude aplikace žije

Tohle je ta část, kterou nemá nikdo. Kombinuj zdroje sestupně podle spolehlivosti a **každou cestu označ zdrojem + confidence**:

| Zdroj | Jak | Confidence |
|---|---|---|
| **MSI file table** | `MsiEnumComponents` + `MsiGetComponentPath` — přesný seznam **každého** nainstalovaného souboru | `Exact` |
| **MSIX manifest** | `Package.InstalledLocation` + `Package.EffectiveLocation` (kontejnery) | `Exact` |
| **Uninstall registry** | `InstallLocation`, `DisplayIcon`, `UninstallString` | `High` |
| **Heuristika** | viz níže | `Guess` |
| **ETW pozorování** | živé sledování zápisů (režim učení) | `Observed` |

**Heuristické cesty** (zkoušej pro každý pár `{Publisher}` / `{ProductName}` z VERSIONINFO):
```
%LOCALAPPDATA%\{Publisher}\{Product}      %LOCALAPPDATA%\{Product}
%APPDATA%\{Publisher}\{Product}           %APPDATA%\{Product}
%PROGRAMDATA%\{Publisher}\{Product}       %PROGRAMDATA%\{Product}
%USERPROFILE%\.{product_lowercase}        %USERPROFILE%\Documents\{Product}
%LOCALAPPDATA%\Packages\{PackageFamily}   (MSIX kontejner)
HKCU\Software\{Publisher}\{Product}       (registry větev)
HKLM\SOFTWARE\{Publisher}\{Product}
```

**Klasifikace role cesty** (podle názvu adresáře, jednoduchá heuristika):
`Install` / `Config` / `Data` / `Cache` / `Logs` / `Registry`

**UI zobrazení pro aplikaci Chrome:**
```
Google Chrome                      139.0.7258.67    ● 14 procesů, 2,1 GB
├─ Instalace   C:\Program Files\Google\Chrome\        [MSI]      1,2 GB
├─ Data        %LOCALAPPDATA%\Google\Chrome\User Data [Heuristika] 4,8 GB
├─ Cache       %LOCALAPPDATA%\Google\Chrome\...\Cache [Heuristika] 3,1 GB
└─ Registry    HKCU\Software\Google\Chrome            [Registry]
```

Cesty s confidence `Guess` musí být v UI vizuálně odlišené (např. tečkovaný podtrh) — nesmíte uživateli tvrdit, že něco víte, když to jen hádáte.

### 5.3 Odinstalace a úklid zbytků (mutace — až za validační vrstvou, v5+)

Cíl uživatele: *„odinstaluj to a ať po tom nic nezbyde."* Splníme pocit, ale bezpečně a vratně. **Nikdy force delete.**

**Případ A — aplikace má oficiální uninstaller** (`UninstallString` / `QuietUninstallString`):
1. Spusť oficiální uninstaller, počkej na dokončení (sleduj proces).
2. Po jeho doběhnutí porovnej mapu souborů (kap. 5.2) proti disku → co zbylo.
3. Zbytky zobraz jako seznam se štítkem confidence. **Nemaž je automaticky** — uninstaller mohl nechat data schválně (licence, profily, uživatelské soubory).
4. Uživatel odklikne, co smazat → **vše do koše** (`FOF_ALLOWUNDO`), registrové větve se **zálohují** (export do `.reg`) před smazáním.
5. Co uživatel nesmaže, zůstává evidované **pod aplikací jako „zbytky"** — i po odinstalaci to nástroj drží, dokud se to neuklidí. Přesně jak si přál.

**Případ B — aplikace nemá uninstaller** (typicky portable): viz kap. 5.4 níže — asistované vratné odstranění, **ne** force delete.

### 5.4 Asistované odstranění (náhrada za „force delete")

Když není uninstaller, **neděláme force delete** — ten je zakázaný vzor (viz „Mimo rozsah"). Riziko: heuristická mapa (confidence `guess`) může ukázat i sdílené cesty, force delete registru/služeb stejně nedosáhne „nezbude nic", a chybí nezávislé ověření vlastnictví.

Místo toho **asistované, revidovatelné odstranění** — stejný pocit „nic nezbylo", ale vratné:
1. Ukaž kompletní strom všeho, co k aplikaci patří: soubory, registr, služby, startup položky, naplánované úlohy — každou se štítkem zdroje a confidence.
2. Položky `exact`/`high` předzaškrtnuté; položky `guess` **defaultně nezaškrtnuté** (uživatel je musí vědomě potvrdit).
3. Vše prochází validační vrstvou (kap. 17): preflight ukáže dopad, nezávislý validátor ověří, že cesta nepatří jinam / není systémová.
4. Provedení: soubory **do koše**, registr **do zálohy** (`.reg` export), služby `disable` (ne smazání), vše vratné.
5. Ověření: co se opravdu odstranilo. Neúspěch → FAILED + rollback.

Rozdíl proti „force" je jediný: je to vratné a člověk to viděl. Riziko rozbití systému je pryč.

---

## 6. Ovladače

### 6.1 Inventář — SetupAPI, ne WMI

**NEPOUŽÍVEJ `Win32_PnPSignedDriver`.** WMI dotaz na tuhle třídu trvá **jednotky sekund** a umí zatuhnout.

Použij `SetupDiGetClassDevs` + `SetupDiEnumDeviceInfo` + `SetupDiGetDeviceRegistryProperty`:
- `SPDRP_DEVICEDESC`, `SPDRP_MFG`, `SPDRP_CLASS`, `SPDRP_HARDWAREID`
- Verze ovladače a datum: `SetupDiGetDeviceInstallParams` → INF, nebo `DEVPKEY_Device_DriverVersion`, `DEVPKEY_Device_DriverDate`, `DEVPKEY_Device_DriverProvider`

**Kdy skenovat:** jednou při startu služby + na `WM_DEVICECHANGE` / `CM_Register_Notification`. **Nikdy v cyklu.**

### 6.2 Kontrola aktualizací — POUZE Windows Update

Použij WUA COM API:
```
IUpdateSession → CreateUpdateSearcher()
searcher.ServerSelection = ssWindowsUpdate
searcher.Search("IsInstalled=0 and Type='Driver'")
```

**Explicitní zákaz:** Neimplementuj generický stahovač ovladačů, který tahá INF ze stránek výrobců nebo z třetích stran. Je to bezpečnostní riziko (špatná verze = BSOD), je to vzor typický pro malware/PUP, a antiviry vás za to označí. Vendor nástroje (NVIDIA App, AMD Adrenalin) nabízej k instalaci **přes winget**, ne vlastním downloaderem.

**Rozsah v1 = pouze detekce + notifikace.** Vzhledem k tvrdému požadavku na stabilitu se v1 **neinstaluje nic**. Aplikace jen zjistí přes WUA, že je novější ovladač dostupný, a zobrazí to.

**Opt-in aktualizace (v2), per ovladač.** Aktualizace je vždy volba uživatele u **konkrétního** ovladače — zaškrtávací pole u položky, nikdy hromadně a nikdy automaticky. Důvod: grafické ovladače spravuje řada lidí radši přes oficiální aplikace výrobce (NVIDIA App, AMD Adrenalin), a nástroj jim do toho nesmí sahat. Kdo si u dané položky auto-update nezaškrtne, tomu se nikdy nic nenainstaluje. Instalace pak (`IUpdateDownloader` + `IUpdateInstaller`) výhradně:
- za explicitním zaškrtnutím a potvrzením u té položky,
- s **povinným** bodem obnovení předem (`SRSetRestorePoint`, `BEGIN_SYSTEM_CHANGE`),
- přes standardní WUA pipeline, nikdy ne vlastním stahováním.

Špatný ovladač je jeden z mála způsobů, jak z userspace opravdu shodit systém. Když si nejste jistí, neinstalujte — jen informujte a odkažte na Windows Update.

---

## 7. Startup položky

Šest backendů, každý s vlastní čtecí a zapisovací funkcí:

| # | Zdroj | Cesta |
|---|---|---|
| 1 | Run klíče | `HKLM\...\CurrentVersion\Run`, `RunOnce`, + `Wow6432Node`, + `HKCU` |
| 2 | Startup složky | `%APPDATA%\...\Start Menu\Programs\Startup`, `%PROGRAMDATA%\...\Startup` |
| 3 | Task Scheduler | COM `ITaskService` → tasky s triggerem `LogonTrigger` / `BootTrigger` |
| 4 | Služby | `EnumServicesStatusEx` → `SERVICE_AUTO_START` |
| 5 | MSIX startup tasks | WinRT `StartupTask.GetForCurrentPackage` / manifest |
| 6 | Shell rozšíření | `HKLM\...\Winlogon\Userinit`, `Shell` (jen čtení, jen varování) |

### Zapnutí/vypnutí — nedestruktivně

**Nemaž registry hodnoty.** Windows má na to oficiální mechanismus, který používá i Správce úloh:

```
HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run
HKCU\...\StartupApproved\StartupFolder
HKLM\...\StartupApproved\Run

Hodnota = REG_BINARY (12 bytů)
byte[0] & 0x01 == 1  →  ZAKÁZÁNO
byte[0] == 0x02      →  povoleno
byte[4..12]          →  FILETIME okamžiku zákazu
```

Pro Task Scheduler: `IRegisteredTask.Enabled = false`.
Pro služby: `ChangeServiceConfig` → `SERVICE_DEMAND_START`.

Každou položku spáruj s aplikací (`app_id`) přes stejnou kaskádu identity z kap. 4 → uživatel vidí *„tohle spouští Adobe"*, ne jen `AdobeGCInvoker-1.0`.

**Měření dopadu:** korelace se startup časem. Přes ETW `Microsoft-Windows-Kernel-Process` změř `ProcessStart` → první idle, agreguj přes bootovací sessions → *„tato položka průměrně přidává 2,3 s ke startu"*.

---

## 8. Datový model (SQLite, WAL)

> Kompletní schéma. Tabulky `incident` (kap. 16.4), `audit` (kap. 17.6), `permission` a `permission_use` (kap. 13.4) jsou definované u svých sekcí, ale patří do téhož schématu a téže databáze. Trvalé tabulky (nemazané retencí): `app`, `app_path`, `driver`, `startup_item`, `identity_override`, `sig_cache`, `event`, `incident`, `audit`, `permission`, `permission_use`.

```sql
-- ═══ APLIKACE ═══
CREATE TABLE app (
  id            INTEGER PRIMARY KEY,
  identity_key  TEXT NOT NULL UNIQUE,   -- viz kap. 4.1
  kind          TEXT NOT NULL,          -- 'os' | 'desktop' | 'msix' | 'unknown'
  display_name  TEXT NOT NULL,
  publisher     TEXT,
  version       TEXT,
  install_date  INTEGER,
  icon_blob     BLOB,
  first_seen    INTEGER NOT NULL,
  last_seen     INTEGER NOT NULL
);

CREATE TABLE app_path (
  app_id      INTEGER NOT NULL REFERENCES app(id) ON DELETE CASCADE,
  path        TEXT NOT NULL,
  role        TEXT NOT NULL,   -- install|config|data|cache|logs|registry
  source      TEXT NOT NULL,   -- msi|msix|registry|heuristic|observed
  confidence  TEXT NOT NULL,   -- exact|high|guess|observed
  size_bytes  INTEGER,         -- lazy, počítá se on-demand
  size_ts     INTEGER,
  PRIMARY KEY (app_id, path)
);

-- ═══ PROCESY ═══
CREATE TABLE proc_instance (
  id            INTEGER PRIMARY KEY,
  pid           INTEGER NOT NULL,
  app_id        INTEGER REFERENCES app(id),
  parent_id     INTEGER REFERENCES proc_instance(id),  -- z ETW, spolehlivé
  image_path    TEXT NOT NULL,
  cmdline       TEXT,
  session_id    INTEGER,
  protection    TEXT NOT NULL,   -- critical|protected|system|user
  start_ts      INTEGER NOT NULL,
  end_ts        INTEGER,
  exit_code     INTEGER
);
CREATE INDEX ix_proc_live   ON proc_instance(end_ts) WHERE end_ts IS NULL;
CREATE INDEX ix_proc_app    ON proc_instance(app_id, start_ts);

-- ═══ VZORKY (retenční kaskáda) ═══
-- 1 Hz, retence 1 hodina
CREATE TABLE sample_1s (
  ts        INTEGER NOT NULL,
  proc_id   INTEGER NOT NULL,
  cpu_ns    INTEGER,   -- delta
  ws_kb     INTEGER,
  priv_kb   INTEGER,
  io_r      INTEGER,   -- delta
  io_w      INTEGER,   -- delta
  hard_flt  INTEGER,   -- delta
  PRIMARY KEY (ts, proc_id)
) WITHOUT ROWID;

-- agregát 10 s, retence 7 dní   (avg + max u každé metriky)
CREATE TABLE sample_10s (...);
-- agregát 1 min, retence 1 rok
CREATE TABLE sample_1m  (...);

-- systémové metriky, stejná kaskáda
CREATE TABLE system_1s (
  ts INTEGER PRIMARY KEY,
  cpu_pct REAL, mem_used_mb INTEGER, commit_mb INTEGER,
  disk_qlen REAL, disk_lat_ms REAL, hard_flt_rate INTEGER,
  gpu_pct REAL, thermal_throttle INTEGER,
  cpu_temp_c REAL,          -- NULL když nedostupné (viz kaskáda 15.2)
  cpu_temp_src TEXT,        -- 'hwinfo'|'lhm'|'acpi'|NULL — vždy uveď zdroj
  cpu_clock_mhz INTEGER, cpu_clock_max_mhz INTEGER  -- throttling = clock/max
) WITHOUT ROWID;

-- per-komponentní senzory (GPU, disky), stejná retenční kaskáda
-- oddělené od system_1s, protože komponent může být víc (2 GPU, N disků)
CREATE TABLE sensor_1s (
  ts        INTEGER NOT NULL,
  comp_id   TEXT NOT NULL,   -- 'gpu:0' | 'disk:0' | …
  temp_c    REAL,            -- NULL když nedostupné
  temp_src  TEXT,            -- 'nvml'|'adlx'|'igcl'|'smart'|NULL
  usage_pct REAL,
  clock_mhz INTEGER,
  power_w   REAL,
  PRIMARY KEY (ts, comp_id)
) WITHOUT ROWID;

-- FPS / frame time per proces (jen kreslící procesy), agregát po sekundě
CREATE TABLE fps_1s (
  ts        INTEGER NOT NULL,
  proc_id   INTEGER NOT NULL,
  frames    INTEGER,         -- počet snímků za sekundu
  ft_avg_ms REAL, ft_p95_ms REAL, ft_p99_ms REAL, ft_max_ms REAL,
  dropped   INTEGER,
  PRIMARY KEY (ts, proc_id)
) WITHOUT ROWID;

-- ═══ UDÁLOSTI ═══
CREATE TABLE event (
  id      INTEGER PRIMARY KEY,
  ts      INTEGER NOT NULL,
  kind    TEXT NOT NULL,  -- stall|app_install|app_uninstall|driver_change|
                          -- startup_change|proc_crash|boot|shutdown
  app_id  INTEGER REFERENCES app(id),
  payload TEXT            -- JSON
);
CREATE INDEX ix_event_ts ON event(ts DESC);

-- ═══ OVLADAČE ═══
CREATE TABLE driver (
  id                 INTEGER PRIMARY KEY,
  device_instance_id TEXT NOT NULL UNIQUE,
  device_name        TEXT NOT NULL,
  class              TEXT,
  provider           TEXT,
  version            TEXT,
  driver_date        INTEGER,
  inf_name           TEXT,
  is_signed          INTEGER,
  update_available   TEXT,     -- verze z WUA, NULL = aktuální
  last_checked       INTEGER
);

-- ═══ STARTUP ═══
CREATE TABLE startup_item (
  id       INTEGER PRIMARY KEY,
  source   TEXT NOT NULL,   -- run_hklm|run_hkcu|folder|task|service|msix
  key      TEXT NOT NULL,   -- identifikátor pro zápis zpět
  name     TEXT NOT NULL,
  command  TEXT NOT NULL,
  app_id   INTEGER REFERENCES app(id),
  enabled  INTEGER NOT NULL,
  scope    TEXT NOT NULL,   -- machine|user
  avg_boot_impact_ms INTEGER,
  UNIQUE(source, key)
);

-- ═══ CACHE PODPISŮ ═══
CREATE TABLE sig_cache (
  path    TEXT NOT NULL,
  size    INTEGER NOT NULL,
  mtime   INTEGER NOT NULL,
  subject TEXT,
  product TEXT,
  valid   INTEGER,
  PRIMARY KEY (path, size, mtime)
) WITHOUT ROWID;
```

### Retenční kaskáda (běží každou minutu, v BELOW_NORMAL vlákně)

```
sample_1s   → 1 hodina  → agreguj do sample_10s → smaž
sample_10s  → 7 dní     → agreguj do sample_1m  → smaž
sample_1m   → 1 rok     → smaž
event       → navždy (jsou malé)
```

Bez retence: 300 procesů × 1 Hz × ~40 B ≈ **1 GB/den**. S retencí: **~200 MB celkem, ustálený stav.**

Nastav `PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA wal_autocheckpoint=1000;`

---

## 9. UI

### 9.1 Vyvolání zkratkou — realita

**Požadavek:** UI musí jít vyvolat zkratkou vždy, i když je PC zaseknuté, jako Správce úloh.

**Co Správce úloh doopravdy dělá:** není v kernelu. Běží v userspace s `HIGH_PRIORITY_CLASS`. Jeho jediná výhoda je, že přes Ctrl+Alt+Del jde spustit ze **Secure Desktopu** (winlogon) — a tam se aplikace třetí strany nedostane. Tuhle cestu mít nebudete.

**Co udělat, aby to fungovalo v ~95 % případů:**

1. **UI proces běží od přihlášení**, skrytý. Zkratka nedělá cold start, jen `ShowWindow`. Cold start Tauri = 800+ ms, což je při zaseknutém PC k ničemu.
2. `RegisterHotKey` (default `Ctrl+Shift+Alt+T`) v UI procesu. Registruj **hned po startu**, ne až po načtení webview.
3. UI proces: `SetPriorityClass(HIGH_PRIORITY_CLASS)`, UI vlákno `THREAD_PRIORITY_HIGHEST`.
4. `SetProcessWorkingSetSizeEx(..., QUOTA_LIMITS_HARDWS_MIN_ENABLE)` → nesmí být odswapován.
5. `SetProcessInformation(ProcessMemoryPriority, MEMORY_PRIORITY_NORMAL)` — WebView2 si jinak snižuje prioritu paměti na pozadí.

**Kde to selže:** WebView2 je Chromium. Při skutečné saturaci disku nebo zamrznutí DWM vám nevykreslí. To není chyba implementace, to je vlastnost — a je to **přijaté omezení** (viz „Mimo rozsah"). Garanci dostupnosti dat drží démon, ne UI: co UI během zámrznutí nezobrazí, to démon zaznamená a uživatel uvidí zpětně jako incident (kap. 16).

### 9.2 Obrazovky

Navigace = sekce ze sekce 1, seskupené (Stav / Správa / Settings). Dva prvky jsou globální, napříč všemi obrazovkami:

- **Stav démona v hlavičce.** Trvalý indikátor u názvu (zeleně = služba běží, červeně = neběží). Bez něj je „mrtvá schránka" nerozeznatelná od „vše v pořádku". Levné, zásadní.
- **Badge zdraví na položkách navigace.** Sekce, která volá po pozornosti (SSD na 12 % životnosti, vypnutý firewall, zastaralý ovladač s auto-update, podezřelý proces), nese tečku. Dělá to z ploché navigace živý přehled — to je celý smysl „nástroje, co hlídá".

| Obrazovka | Obsah |
|---|---|
| **Home** | Grid dlaždic — rychlé info z celé aplikace: živé CPU/RAM/disk, poslední zásek, zdraví disků, stav ochrany, počet aplikací/procesů, upozornění |
| **Tasks** | Task manager: strom **aplikace → procesy**, sloupce CPU/RAM/IO/síť, ochranné třídy odlišené. **Vestavěná časová osa + historie + detail záseku** s viníkem |
| **Apps** | Seznam nainstalovaných + verze + ringy, ve kterých běží. Detail = strom mapy souborů (kde všude aplikace žije) + běžící procesy. Odinstalace + úklid zbytků + evidence „zbytků" |
| **Files** | Disky, oddíly, SMART zdraví. Rychlý MFT prohlížeč s vyhledáváním, barevné rozlišení systémové/skryté/obyčejné. Duplicity on-demand. Mazání se závislostmi |
| **Start** | Startup položky seskupené podle aplikace. Přepínač zapnuto/vypnuto. Odhad dopadu na boot |
| **Users** | Účty, oprávnění (kdo je admin), historie přihlášení |
| **Hardware** | **Komponentově (kap. 15.4):** karta každé komponenty (CPU, GPU, disky, RAM, baterie) = nahoře živý+historický graf, pod ním údaje té komponenty. GPU vč. teploty (NVML/ADLX/IGCL), CPU teplota dle kaskády se zdrojem, FPS/frame time |
| **Drivers** | Tabulka: zařízení, verze, datum, podpis. Opt-in aktualizace per ovladač (checkbox) |
| **Network** | Spojení per aplikace/proces, otevřené porty, kam vede trafik (IP, geo), objem v čase, WiFi. Signály podezřelého trafiku |
| **Security** | Stav ochrany (Defender, firewall, BitLocker, Secure Boot, TPM). **Oprávnění aplikací (kap. 13.4)** — kdo má přístup ke kameře/mikrofonu/poloze, **kdo je používá právě teď** (živá tečka), historie použití. Signály podezřelých procesů. Telemetrie a soukromí + přepínače |
| **Incidenty** | Pády aplikací, hangy, BSODy, záseky pod jedním modelem. Detail = časová osa pádu + karta každé komponenty s křivkou v okně (kap. 16.4). Viník + bugcheck/modul |
| **Settings** | Konfigurace nástroje (intervaly, retence, zapnuté kolektory, zkratka, striktní režim). Dlaždice **„Spotřeba nástroje"** (kap. 2.3) — vlastní CPU/RAM/zápis, aby si uživatel ověřil rozpočet |

### 9.3 Grafy

**Použij [uPlot](https://github.com/leeoniya/uPlot), ne Chart.js.** uPlot vykreslí 100k bodů za ~20 ms; Chart.js na tom umře. Časové řady jsou celý smysl téhle aplikace.

- Načítej z odpovídající agregační tabulky podle zoomu (1s/10s/1m) — nikdy nedotahuj rok surových dat.
- Živé grafy: streamuj přes named pipe, ne polling.
- **Komponentový princip (napříč celým UI, kap. 15.4):** kde se ukazuje výkon nějaké entity (komponenta, proces, aplikace), platí vždy stejné schéma — **nahoře graf (živý i historický přes stejný přepínač času), pod ním textové údaje té entity.** Historie není samostatná obrazovka, je to časová osa uvnitř karty. Všechny informace o jedné entitě jsou pohromadě u ní.

### 9.4 Design (dle preferencí)

- Tech + minimalismus, tmavý základ, glassmorphism, jemné borders.
- Akcenty `#FFAA00` a `#FF6E00` — **střídmě**, jen pro upozornění a aktivní stavy.
- Font: Tiempos Headline (nadpisy) + Inter (text).
- Border radius 4–8 px. Ikony: Lucide (outline).
- SPA, žádné přenačítání. Animace svižné.
- Barevné kódování ochranných tříd: kritické = šedá + zámek, systémové = jantarová, uživatelské = neutrální.
- Barevné kódování vynucení oprávnění (kap. 13.4): zeleně = Windows tvrdě vynutí (MSIX), jantarově = odepřeno, ale nevynuceno (Win32). **Nikdy nepoužívej zelenou tam, kde vynucení není.**

### 9.5 První spuštění — kritické nastavení, nikdy potichu

Po instalaci se otevře **obrazovka prvního spuštění**. Nástroj, který hlídá soukromí uživatele, musí být sám vzorem: **nic si nezapne mlčky.** Každá položka je vysvětlená a odklikaná.

| Položka | Default | Proč to uživatel musí vědět |
|---|---|---|
| **Služba při startu systému** | zapnuto | *„Bez toho nemá nástroj historii ani incidenty — monitoring poběží jen když ho ručně spustíte."* Toto je **kritické nastavení**: bez něj nástroj ztrácí svou hlavní hodnotu. |
| **UI při přihlášení** (skryté, pro zkratku) | zapnuto | Levné (~pár MB), umožní vyvolat okno zkratkou bez cold startu. |
| **Klávesová zkratka** | `Ctrl+Shift+Alt+T` | Lze změnit nebo vypnout. |
| **Striktní režim** (bod obnovení před nevratnými akcemi) | zapnuto | Pojistka pro T1 akce. Doporučeno nechat. |
| **Které kolektory** (síť, senzory, incidenty) | zapnuto | Vypnutelné jednotlivě, pokud uživatel nechce. |
| **Učící režim mapy souborů** (ETW FileIo) | **vypnuto** | Zapíná se vědomě — je to jediný kolektor s citelnějším nákladem. |

Instalátor (MSI) službu **zaregistruje** (potřebuje elevaci), ale **režim spouštění** je volba uživatele při prvním spuštění: `SERVICE_AUTO_START` (default, po potvrzení) vs. `SERVICE_DEMAND_START`.

Toto nastavení je později kdykoli změnitelné v Settings a **objeví se i v sekci Start** jako běžná startup položka (kap. 2.3) — viditelná, vypnutelná, jen s varováním, co tím uživatel ztratí.

---

## 10. IPC protokol

Named pipe `\\.\pipe\syswatch`, message mode, délkově prefixované rámce, serializace **postcard** (menší a rychlejší než JSON).

```rust
// core-types/src/ipc.rs
pub enum Request {
    // ── DOTAZY (čtení, bez validace) ──
    Subscribe { stream: StreamKind },     // live procesy / systém / senzory
    Unsubscribe { stream: StreamKind },
    QueryHistory { from: i64, to: i64, resolution: Res },
    QueryApps,
    QueryAppDetail { app_id: i64 },
    QueryDrivers,
    CheckDriverUpdates,
    QueryStartup,
    QueryFileLock { path: String },       // kdo drží soubor (jen čtení)
    QueryAudit { from: i64, to: i64 },    // historie mutací (kap. 17.6)
    QueryPermissions,                     // oprávnění všech aplikací + kdo právě používá (kap. 13.4)
    QueryPermissionHistory { app_id: i64, capability: String, from: i64, to: i64 },
    QuerySelfUsage,                       // vlastní spotřeba nástroje (kap. 2.3) — ověřitelnost

    // ── T0: RYCHLÉ VRATNÉ AKCE (validace odlehčená, bez potvrzení, < 50 ms) ──
    // Provedou se přímo, projdou validační vrstvou v odlehčeném režimu (kap. 17.2).
    ToggleStartup   { item_id: i64, enabled: bool },
    TogglePrivacy   { key_id: i64, enabled: bool },   // přepínač soukromí/telemetrie
    TogglePermission { app_id: i64, capability: String, allow: bool }, // ConsentStore (kap. 13.4)
    ToggleDriverOptIn { driver_id: i64, enabled: bool },
    SetLearningMode { enabled: bool },

    // ── T1: TĚŽKÉ AKCE (dvoufázově: nejdřív Plan*, po potvrzení Execute*) ──
    PlanKill    { pid: u32, instance_id: i64 },
    PlanDelete  { path: String, permanent: bool },
    PlanUninstall { app_id: i64 },
    Execute     { plan_id: u64 },          // provede dřív vrácený a stále platný plán
    CancelPlan  { plan_id: u64 },
}

pub enum Response {
    // dotazy
    ProcSnapshot(Vec<ProcRow>),
    SystemSnapshot(SystemRow),
    History(Vec<Bucket>),
    Apps(Vec<AppRow>),
    AppDetail(AppDetail),
    Drivers(Vec<DriverRow>),
    Startup(Vec<StartupRow>),
    FileLock(Vec<LockHolder>),          // kdo drží + třída (critical/service/user)
    Audit(Vec<AuditRow>),
    Permissions(Vec<PermissionRow>),    // + enforced: bool, in_use: bool (kap. 13.4)
    PermissionHistory(Vec<PermUseRow>),
    SelfUsage(SelfUsageRow),            // CPU/RAM/zápis/velikost DB nástroje samotného

    // T0 výsledek (rovnou)
    ToggleOk { reversible: String },    // jak vrátit, pro „undo" v UI
    ToggleDenied { reason: String },

    // T1 dvoufázově
    Plan { plan_id: u64, expires_ts: i64, steps: Vec<PlanStep>, warnings: Vec<String> },
    ExecuteOk { audit_id: i64 },        // odkaz do auditu
    ExecuteFailed { reason: String, rolled_back: bool },
    Denied { reason: String },          // validace zamítla (např. "Kritický systémový proces")

    Error { message: String },
}
```

Dvě věci, které protokol vynucuje:

- **`instance_id` u killu**, ne jen PID — mezi zobrazením v UI a kliknutím mohl PID zaniknout a být recyklován. Validátor ověří, že `instance_id` stále sedí (kap. 17.3), jinak `Denied`.
- **`expires_ts` u plánu** — plán z `PlanDelete`/`PlanKill` má omezenou platnost. `Execute` po vypršení validátor odmítne a přinutí přeplánovat, protože svět se mezitím mohl změnit. Plán není souhlas na neomezenou dobu.

---

## 11. Files — disky, prohlížeč, mazání se závislostmi

Sekce spojuje čtení (přehled disků, prohlížeč, hledání) a mutaci (mazání). Mazání se závislostmi je nejrizikovější operace projektu → až v8, celé přes validační vrstvu z kap. 17 a mechaniku z kap. 18.

### 11.1 Přehled disků (čtení, levné)

- Fyzické disky, oddíly, souborový systém, volné/obsazené místo — `GetLogicalDrives`, `DeviceIoControl(IOCTL_DISK_GET_DRIVE_LAYOUT_EX)`, `GetDiskFreeSpaceEx`.
- **Zdraví disku (SMART)** přes `IOCTL_STORAGE_QUERY_PROPERTY` / `IOCTL_STORAGE_PREDICT_FAILURE` — bez vlastního driveru, jen admin. Nejcennější položka: *„SSD má 94 % životnosti"*, reallocated sectors, teplota disku (ta z SMART je čitelná, na rozdíl od CPU/GPU).
- Tato podsekce sama o sobě je minimální bezpečný režim, kdyby se prohlížeč (11.2) rozhodl odložit.

### 11.2 Prohlížeč souborů — na NTFS MFT/USN, ne vlastní index

**Nikdy nestav full-text index obsahu disku** — rozbil by rozpočet (GB indexu, hodiny skenu).

Vyhledávání podle jména/cesty/velikosti/data staví na **NTFS Master File Table** čtené přes `USN Journal` (`FSCTL_ENUM_USN_DATA` / `FSCTL_QUERY_USN_JOURNAL`). Tak funguje Everything: celý svazek za sekundy, index vede sám filesystém, náklad skoro nulový. USN journal navíc dává **živé změny** — po prvním načtení MFT stačí číst přírůstky.

- Rychlý listový průzkumník, vyhledávání napříč celým PC (instant, z MFT).
- **Barevné rozlišení** souborů: systémové (`FILE_ATTRIBUTE_SYSTEM`), skryté (`FILE_ATTRIBUTE_HIDDEN`), obyčejné — jasně odlišené, systémové s varováním.
- Fallback pro ne-NTFS svazky (FAT32, exFAT, síťové): klasické `FindFirstFile` enumerace, bez instant search (a řekni to uživateli).

### 11.3 Duplicitní soubory — on-demand, dvoufázově

**Ne stálý index, ne percepční podobnost.** Jen bit-identické duplicity, spuštěné uživatelem na vybraném rozsahu (složka/disk):
1. **Fáze 1 (skoro zdarma):** seskup podle velikosti z MFT. Unikátní velikost → nemůže mít duplikát, vynech.
2. **Fáze 2 (jen kolize):** u souborů se shodnou velikostí spočítej hash (BLAKE3). Nejdřív hash prvních 4 KB, teprve při shodě celý soubor — hashuje se tak ~1 % souborů.

Běží jako **úloha se stavem a progresem**, ne na pozadí. Výsledek: skupiny duplicit, uživatel vybere, co smazat (do koše). „Podobné, ne identické" soubory jsou mimo rozsah.

### 11.4 Mazání se závislostmi (mutace, v8)

Jádro popsané v kap. 18. Z pohledu Files: když mazání blokuje zámek, nabídne se bezpečné řešení (ukončit/uspat držitele → smazat → obnovit), vše přes validaci. Default do koše.

---

## 12. Network — spojení per aplikace, porty, trafik

Nejambicióznější čtecí sekce. Půlka levná a unikátní, půlka (obsah paketů) mimo rozsah.

### 12.1 Co dělat (levné, unikátní)

- **Spojení per proces/aplikace:** `GetExtendedTcpTable` / `GetExtendedUdpTable` s `TCP_TABLE_OWNER_PID_ALL` → každé spojení nese PID → napojení na seskupení aplikací (kap. 4). *„Chrome má 40 spojení, kam vedou."* Task Manager tohle per-aplikaci neumí.
- **Otevřené porty a brány** (listening), lokální/vzdálená IP a port, stav spojení.
- **Kam to vede:** reverzní DNS, geolokace přes **offline** databázi (MaxMind GeoLite2 lokálně, ne online lookup — soukromí + rychlost).
- **Objem trafiku per aplikace v čase** — přes ETW `Microsoft-Windows-Kernel-Network`, agregace do historie (stejná retenční kaskáda jako procesy).
- **WiFi:** seznam sítí, síla signálu, připojení/zapomenutí přes `WlanAPI`. (Poznámka: WiFi mísí čtení a správu — drž správu za potvrzením, čtení volně.)

### 12.2 Co nedělat

- **Obsah paketů (DPI).** Vyžaduje `WinDivert`/WFP callout **driver** (kernel), a u TLS je obsah šifrovaný — stejně neuvidíš nic než cíl. Mimo rozsah. Ukazuj **kam a kolik**, ne **co**.
- **Detekce podezřelého trafiku** drž jako **signály, ne verdikt:** proces bez okna komunikuje ven, spojení do neobvyklé země, port typický pro malware. Formulace „tohle je neobvyklé, tady je proč", ne „tohle je útok". Je to heuristika nad daty, co už máš.

### 12.3 Náklad

Snapshot tabulky spojení 1×/s je levný. Reverzní DNS a geo cachuj (jako podpisy). ETW Network filtrovaně. Vejde se do rozpočtu.

---

## 13. Security — stav ochrany, soukromí, signály

Informační sekce + vratné přepínače soukromí. **Žádný vlastní antivirus, žádné verdikty.**

### 13.1 Stav vestavěné ochrany (čtení)

Jedna obrazovka „jsem chráněný?": Defender (zapnutý, aktuální definice, poslední sken), firewall (per profil), BitLocker, Secure Boot, TPM, UAC úroveň, stav Windows Update. Vše přes veřejná API (`Windows Security Center` WMI `root\SecurityCenter2`, `BitLocker` WMI, `Tpm` API) — prakticky zdarma, jen čtení.

### 13.2 Signály podezřelých procesů (ne sken malwaru)

Levné, poctivé **red flags** — nikdy ne „tohle je malware":
- nepodepsaný proces běžící z `%TEMP%` / `%APPDATA%`,
- jméno předstírající systémový proces (`svch0st.exe`, `csrss ` s mezerou),
- spuštění z neobvyklého místa, chybějící popis/podpis.

Zobraz s formulací *„vypadá neobvykle, tady je proč"* + odkaz „prověřit v Defenderu". Ten rozdíl chrání před falešnými poplachy i před odpovědností.

### 13.3 Soukromí a telemetrie (čtení + vratné přepínače)

- Přehled telemetrie a soukromí Windows + **přepínače všeho, co jde v uživatelském rozsahu** (telemetrie level, reklamní ID, aktivita, atd.) — vratné, přes validační vrstvu (T0).
- **„Kam aplikace volá":** z tvých **vlastních** síťových dat (kap. 12), ne z převzatého hodnocení.

**Formulace o sběru dat:** drž se **faktů, které nástroj sám změří** — jaká oprávnění si aplikace bere, na jaké endpointy pozorovaně volá, co startuje. Nikdy ne převzaté hodnocení „aplikace X sbírá data" (právně citlivé, nemáš zdroj). Pozoruješ → tvrdíš. Nepozoruješ → mlčíš.

### 13.4 Oprávnění aplikací — kdo má co a kdo to používá právě teď

Vlajková část Security. Cíl: **jedna obrazovka, kde uživatel vidí, která aplikace má přístup ke kameře, mikrofonu, poloze, souborům — a která je používá právě v tuto chvíli.**

#### Zdroj dat: CapabilityAccessManager ConsentStore

Windows drží per-aplikaci souhlasy i **časy použití** v registru:

```
HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\
  <capability>\                    # webcam, microphone, location, contacts,
                                   # appointments, email, phoneCall, documentsLibrary,
                                   # picturesLibrary, videosLibrary, broadFileSystemAccess…
    <PackageFamilyName>\           # balené (MSIX/UWP) aplikace
        Value              = "Allow" | "Deny"
        LastUsedTimeStart  = FILETIME
        LastUsedTimeStop   = FILETIME   ← 0 znamená: POUŽÍVÁ PRÁVĚ TEĎ
    NonPackaged\
      <cesta k .exe, '\' nahrazeno '#'>\
        Value, LastUsedTimeStart, LastUsedTimeStop
```
(Obdoba i v `HKLM` pro nastavení na úrovni stroje.)

- **Živý indikátor „právě používá"** = `LastUsedTimeStop == 0` při nenulovém `LastUsedTimeStart`. Přesně tohle pohání tečku u kamery ve Windows i v macOS.
- **Událostně, ne pollem:** `RegNotifyChangeKeyValue` na klíč → reaguješ na změnu, žádný cyklus. Cena prakticky nulová.
- **Přepínač Allow/Deny** = zápis do `Value` — týž klíč, který zapisuje aplikace Nastavení. Třída **T0** (vratné) přes validační vrstvu.

#### ⚠ Vynucení: Windows NENÍ macOS — a musíš to říct nahlas

Tohle je nejdůležitější věta celé sekce. **macOS TCC je mandatorní řízení přístupu vynucené jádrem** — Deny znamená, že se aplikace ke kameře fyzicky nedostane. **Windows to takhle nemá.**

| Typ aplikace | Co „Deny" doopravdy znamená | Zobrazení v UI |
|---|---|---|
| **MSIX / UWP** (balené) | Windows to **tvrdě vynutí** přes broker. Aplikace se nedostane. | 🛡️ zelená: **„Zablokováno"** |
| **Win32 desktop** (nebalené) | Windows to **požaduje, ale tvrdě nevynucuje**. Aplikace může sáhnout na kameru přes DirectShow / ovladač výrobce a nastavení obejít. | ⚠️ jantarová: **„Odepřeno, ale Windows to u klasických aplikací nevynucuje"** |

**Nikdy nebuduj dojem tvrdého zámku tam, kde není.** Kdyby uživatel věřil, že je chráněný, a nebyl, je to horší, než kdybys nezobrazil nic. Upřímnost o hranici vynucení **je** ta bezpečnostní funkce — ne její oslabení.

#### Kde Mac naopak překonáš: historie použití

macOS ukáže „mikrofon se nedávno používal". Ty máš časové řady — takže můžeš ukázat:

> *„Discord používal mikrofon včera 3 h 12 min, naposledy ve 21:40."*
> *„Neznámá aplikace sáhla na kameru ve 3:14 ráno."*

To je **skutečné hlídání**, ne jeho dojem, a nikdo jiný to na Windows nedělá. Vzniká zdarma: události ze `ConsentStore` se logují do `permission_use` (viz kap. 8) a agregují stejnou retenční kaskádou.

#### Datový model

```sql
-- statický stav oprávnění (snapshot, obnovuje se událostně)
CREATE TABLE permission (
  app_id     INTEGER NOT NULL REFERENCES app(id) ON DELETE CASCADE,
  capability TEXT NOT NULL,        -- webcam | microphone | location | …
  value      TEXT NOT NULL,        -- allow | deny
  enforced   INTEGER NOT NULL,     -- 1 = MSIX (tvrdě vynuceno), 0 = Win32 (poradní)
  in_use     INTEGER NOT NULL,     -- 1 = právě teď (LastUsedTimeStop == 0)
  last_used  INTEGER,
  PRIMARY KEY (app_id, capability)
);

-- historie použití (odtud „3 h 12 min včera")
CREATE TABLE permission_use (
  id         INTEGER PRIMARY KEY,
  app_id     INTEGER NOT NULL REFERENCES app(id),
  capability TEXT NOT NULL,
  start_ts   INTEGER NOT NULL,
  stop_ts    INTEGER            -- NULL = stále používá
);
CREATE INDEX ix_permuse ON permission_use(app_id, capability, start_ts DESC);
```

Obojí je trvalé (nemaže se retencí — objem je nepatrný).

#### UI

Dva pohledy nad týmiž daty:
- **Podle oprávnění:** „Kamera → 3 aplikace mají přístup, Zoom ji používá právě teď."
- **Podle aplikace:** karta aplikace = její oprávnění + kdy je naposledy použila + graf využití v čase (komponentový princip, kap. 15.4).

Aplikace používající oprávnění **právě teď** nese živou tečku — v seznamu i v badge navigace (kap. 9.2).

---

## 14. Users — účty a oprávnění (čtení)

Levná čtecí sekce:
- Lokální účty (`NetUserEnum`), skupiny a členství (`NetLocalGroupEnum`) → **kdo má admin práva**.
- Typ účtu, stav (aktivní/zamčený/vypršelý), poslední přihlášení.
- Historie přihlášení z Security event logu (`EvtQuery` na `Security` kanál, event 4624/4625) — čtení, žádná mutace.

Hodnota pro laika: *„tenhle účet má admin práva a naposledy se přihlásil…"*. Správa účtů (vytváření, změna práv) je mimo rozsah — na to jsou nástroje Windows.

---

## 15. Hardware — inventář, senzory a stav (čtení)

### 15.1 Inventář (levné, jednorázově)

Inventář přes WMI (jednorázově, ne v cyklu) + SetupAPI:
- CPU (model, jádra, takt), RAM (moduly, sloty, rychlost, výrobní čísla), GPU, základní deska, BIOS/UEFI verze.
- Disky (viz 11.1, sdílený kód) včetně **SMART zdraví**.
- Sériová/výrobní čísla, kde jsou čitelná.
- **Stav, kde existuje:** SMART u disků, stav baterie (`GetSystemPowerStatus`, cykly, opotřebení) u notebooků.

### 15.2 Teploty — degradační kaskáda (dřívější „nelze" byl chybný závěr)

**GPU teploty JSOU z userspace dostupné, oficiálně a levně** — přes SDK výrobců, které mluví s už nainstalovaným ovladačem, žádný vlastní kernel driver:

| Výrobce | API | Poskytuje |
|---|---|---|
| NVIDIA | **NVML** (`nvidia-ml.dll`), `nvmlDeviceGetTemperature` | teplota, takty, spotřeba, využití — dokumentované, stabilní |
| AMD | **ADLX** (AMD Display Library eXtended) | teplota, takty, ventilátory |
| Intel Arc | **IGCL** / Level Zero sysman | teplota, telemetrie |

NVML na 1 Hz je prakticky zdarma. Tohle **dělej** — u GPU je teplota plnohodnotná metrika.

**CPU teploty — degradační kaskáda, nikdy nepředstírej číslo, které nemáš:**

| Priorita | Zdroj | Dostupnost | Poznámka |
|---|---|---|---|
| 1 | **HWiNFO** shared memory / **LibreHardwareMonitor** WMI (`root\LibreHardwareMonitor`), pokud běží | přesné | driver si nainstaloval **uživatel** jiným nástrojem — tvoje app zůstává čistá, jen konzumuje |
| 2 | **ACPI thermal zone** (`MSAcpi_ThermalZoneTemperature`, `root\WMI`) | často notebooky, zřídka desktopy | userspace + admin, bez driveru; desktopy často hlásí chipset, ne jádra |
| 3 | **Throttling + takty** (`CallNtPowerInformation(ProcessorInformation)`, aktuální vs. max MHz) | **vždy, 100 % strojů** | odpovídá na skutečnou otázku „zpomaluje mi to kvůli teplu?" i bez stupňů |

**Nikdy neshipuj vlastní kernel driver** (WinRing0 je v Microsoftím blocklistu → nástroj by byl blokován; vlastní driver = EV cert + attestation, samostatný projekt, popírá „nesmí destabilizovat systém"). Kaskáda výše žádný nepotřebuje.

**Náklad:** NVML/ADLX 1 Hz zdarma. WMI ACPI je pomalé → poll po 5–10 s a cachuj, ne v sekundovém cyklu. Stupeň 3 máš stejně už z detekce záseků (kap. 3.3).

### 15.3 FPS / frame time — přes ETW, bez injektáže

FPS se měří **bez hooků, bez driveru, bez injektáže** — přes ETW, tedy infrastrukturou, kterou už stavíš. Každé `Present()` generuje událost v `Microsoft-Windows-DXGI` a `Microsoft-Windows-Dwm-Core`. Odchytáváš system-wide, per proces. Přesně tak funguje PresentMon (Intel, MIT).

Co z toho:
- **Frame time v ms** per aplikace (cennější než průměrné FPS).
- **Frame time spiky = mikro-záseky** → napojení na detekci záseků (kap. 3.3), stejná logika, jemnější rozlišení.
- Present mode (fullscreen/kompozitní), dropped frames.

**Dvě pravidla nákladu:**
1. **Neukládej každý snímek.** Při 240 FPS = 240 řádků/s/proces. Agreguj už v kolektoru: za sekundu ulož počet snímků + min/avg/p95/p99/max frame time.
2. **Zapínej cíleně** jen pro procesy, které aktivně kreslí přes GPU — ne globálně.

### 15.4 Komponentově orientované UI (klíčový princip zobrazení)

**Veškeré informace o jedné hardwarové komponentě jsou pohromadě u té komponenty.** Ne rozházené mezi obrazovkami. Každá komponenta (CPU, GPU, každý disk, RAM, baterie) je karta, která obsahuje **v tomto pořadí shora dolů**:

```
┌─ GPU — NVIDIA RTX 4070 ──────────────────────────┐
│  [ živý graf: teplota + využití + takty ]         │  ← graf výkonu (živý)
│  [ historie: přepínač 1h / 24h / 7d / 30d ]       │  ← tentýž graf v historii
│  ───────────────────────────────────────────────  │
│  Teplota      54 °C        (zdroj: NVML)          │  ← informace pod grafem
│  Využití      12 %                                 │
│  Takt         1 815 MHz / max 2 610 MHz           │
│  Spotřeba     45 W / 200 W                         │
│  Throttling   ne                                   │
│  VRAM         2,1 / 12 GB                          │
│  Ovladač      552.44  (viz Drivers)               │
└───────────────────────────────────────────────────┘
```

Pravidlo: **graf nahoře (živý i historický přes stejnou komponentu), pod ním textové údaje té komponenty.** Historie není samostatná obrazovka — je to časová osa uvnitř karty komponenty. Uživatel klikne na GPU a vidí všechno o GPU: jak si vede teď, jak si vedlo, a jaké má parametry. Stejně CPU, disky, RAM.

Tentýž princip platí i pro incidenty (kap. 16): v detailu incidentu se u každé komponenty ukáže její křivka v okně kolem pádu.

---

## 16. Incidenty — záseky, pády aplikací a BSODy pod jedním modelem

Sjednocující koncept: **zásek, mikro-stutter, pád aplikace i BSOD mají stejný tvar** — časové razítko, časová osa okolo, viník. Sdílí datový model (`event` + nová `incident`) i UI (detail incidentu). Tohle je největší diferenciátor nástroje: Prohlížeč událostí řekne „spadlo to", tvůj nástroj řekne „spadlo to a tady je CPU, disk, RAM, teploty a frame time za 5 minut před pádem — a tenhle proces žral disk". **Protože jsi u toho byl a nahrával.**

### 16.1 Tři typy incidentů, tři zdroje (všechny levné)

| Typ | Detekce | Zdroj |
|---|---|---|
| **Pád aplikace** | ETW `ProcessStop` s exit code (`0xC0000005` access violation atd.) + Event Log `Application` 1000/1001 (WER) pro detail | máš zdarma z v3 |
| **Zaseknutá aplikace** | Event `Application` 1002 (App Hang) + `IsHungAppWindow()` | levné |
| **BSOD / tvrdý pád PC** | po restartu: nový `%SystemRoot%\Minidump\*.dmp` + Event Log `System` 1001 (BugCheck, kód+parametry) + 6008 / Kernel-Power 41 (nečekané vypnutí i bez dumpu) | levné |

Bugcheck kód přelož na lidskou příčinu ze **statické tabulky** (`0x0000009F` = ovladač v chybném stavu při přechodu do spánku, atd.). Chybující modul jde z hlavičky minidumpu — **bez debuggeru**.

### 16.2 Automatické generování při startu

Při startu služby zkontroluj: bylo poslední vypnutí čisté? Je nový minidump? Když ne → **automaticky vytvoř incident** s bugcheck kódem, chybujícím modulem a nahranou časovou osou z historie.

### 16.3 Černá skříňka — autologger povýšen na POVINNÝ

Dřív volitelný ETW autologger je teď **povinný**, protože má konkrétní forenzní účel.

Problém: SQLite se `synchronous=NORMAL` může při BSODu ztratit poslední flush (2 s) — tedy přesně okamžik pádu. Řešení: **ETW autologger session ve file módu** (`.etl`). Buffery zapisuje **jádro, ne tvůj proces** → přežije BSOD i hladovění služby. Rotující ring 64 MB. Po restartu se `.etl` z okna pádu naparsuje do incidentu.

Tím máš data z kritické vteřiny i tehdy, když se SQLite flush nestihl.

### 16.4 UI incidentu (komponentově, dle kap. 15.4)

Detail incidentu = časová osa s okamžikem pádu uprostřed, a pod ní **karta každé komponenty** s její křivkou v okně T-5min .. T+30s: CPU, GPU (vč. teploty), RAM, každý disk (I/O, latence), frame time. Nahoře viník (proces / modul / bugcheck), pod ním komponenty. Incident je trvalý (nemaže se retencí).

```sql
CREATE TABLE incident (
  id          INTEGER PRIMARY KEY,
  ts          INTEGER NOT NULL,
  kind        TEXT NOT NULL,   -- app_crash | app_hang | bsod | stall
  app_id      INTEGER REFERENCES app(id),
  culprit     TEXT,            -- proces / modul / bugcheck kód
  detail      TEXT,            -- JSON: exit code, bugcheck params, faulting module
  etl_path    TEXT,            -- cesta k .etl výřezu z okna pádu
  window_from INTEGER,         -- začátek forenzního okna
  window_to   INTEGER
);
CREATE INDEX ix_incident_ts ON incident(ts DESC);
```

---

## 17. Validační vrstva — srdce bezpečnosti celé aplikace

Tohle je **nejdůležitější komponenta projektu**. Každá změna stavu systému, od smazání souboru po přepnutí jednoho startup přepínače, prochází tudy. Je to jediná brána mezi UI a systémem. Proto má tři tvrdé vlastnosti: je **samostatná**, je **rychlá** a je **neprůstřelná**.

### 17.1 Samostatnost (proč je to vlastní crate)

`validate/` je izolovaná crate závislá jen na `core-types` a `win-sys` (viz pravidlo závislostí v kap. 2.2). Důsledky, které jsou záměrné:

- **Jde zkompilovat a otestovat bez zbytku aplikace.** Testy validátoru neimportují žádný kolektor ani exekutor. Testuješ ho proti umělým i živým stavům OS, ne proti vlastní implementaci akcí.
- **Nezná exekutory.** Validátor rozhoduje *zda* je akce přípustná; *jak* se provede, je věc `actor-*`. Kdyby je znal, mohl by validovat proti jejich předpokladům místo proti realitě OS.
- **Jeden vstupní bod.** `validate(action: &Action, ctx: &LiveContext) -> Verdict`. Neexistuje druhá cesta, jak akci schválit. Žádný `actor-*` se nesmí spustit bez `Verdict::Allow`.

### 17.2 Rychlost a lehkost — dvě třídy akcí

Ne každá akce nese stejné riziko, takže ne každá potřebuje stejně těžkou validaci. Rozlišujeme dvě třídy, aby **rychlé přepínače nastavení byly opravdu rychlé** a těžké operace opravdu bezpečné:

| Třída | Akce | Validace | Cíl latence | Potvrzení |
|---|---|---|---|---|
| **T0 — rychlá, vratná** | startup on/off, přepínač soukromí, driver opt-in checkbox | odlehčená: cíl existuje? je zápis vratný? má volající práva? | **< 50 ms** | žádné (akce je triviálně vratná) |
| **T1 — těžká, riziková** | kill procesu, mazání souboru, odinstalace | plná kaskáda (17.4) + preflight + případně bod obnovení | < 500 ms | vždy, s preflightem |

T0 je navržená tak, aby přepnutí přepínače v UI bylo **okamžité**: validace je pár čtení stavu, akce je jeden vratný zápis (např. `StartupApproved` bajt), potvrzení netřeba, protože zpět to jde stejným přepnutím. Uživatel klikne, přepínač se přepne, hotovo — žádný dialog. Přesto to jde plnou vrstvou (audit, ověření), jen v odlehčeném režimu.

T1 nikdy nezrychluj na úroveň T0. Rozdíl v riziku je zásadní: přepnutí startupu se vrátí jedním kliknutím, smazaný soubor nebo zabitý proces ne.

**Lehkost samotné vrstvy:** validace je čistě on-demand. Když se nic nemění, `validate/` **nespotřebovává nic** — žádné vlákno na pozadí, žádný poll, žádná paměť nad rámec staticky nutného. To je klíč k tomu, aby přítomnost mutující cesty nezdražila 24/7 běh démona. Horký rozpočet (< 0,5 % CPU) se týká sběru; validace do něj nespadá, protože v klidu neběží.

### 17.3 Čerstvý stav, ne snapshot z UI

Validátor **nikdy nevěří tomu, co zobrazuje UI.** UI vidí snapshot starý až 1 s; mezitím se svět změnil. Validátor si proto pro každou akci načte **živý stav OS v okamžiku validace**:

- proces: žije *teď*? sedí `instance_id` (ne jen PID — ten se recykluje)?
- soubor: existuje *teď*? kdo ho drží *teď* (Restart Manager)?
- třída ochrany: čerstvá kontrola (`ProcessBreakOnTermination`), **ne** z cache identity.

Tím je vrstva odolná i proti race conditions i proti zastaralému nebo podvrženému požadavku z UI.

### 17.4 Čtyři fáze (plná kaskáda pro T1)

```
1. PLÁN      Exekutor sestaví seznam kroků + předpoví důsledky. Nic nemění.
             Vrátí se do UI jako Response::Plan.
             Např.: "Smazání vyžaduje ukončit 3 procesy: chrome (2136),
                     chrome (4820), updater (990)."

2. VALIDACE  Nezávislý validátor ověří KAŽDÝ krok proti ŽIVÉMU stavu (17.3):
             - cíl stále existuje?
             - není cíl kritický/protected? (čerstvá kontrola, ne cache)
             - má volající práva? (kontrola tokenu klienta named pipe)
             - je krok vratný, nebo vyžaduje checkpoint?
             Selže-li kterýkoli → celá akce ZAMÍTNUTA, nic se neprovede.

3. PROVEDENÍ Až po potvrzení uživatelem (u T1). Kroky v pořadí, transakčně.
             Selže-li krok uprostřed → STOP + rollback předchozích kroků
             (obnovit uspané procesy, re-enable služby).

4. OVĚŘENÍ   Po provedení se znovu zkontroluje výsledek proti živému stavu:
             soubor opravdu zmizel? proces opravdu skončil?
             Neúspěch → akce označena FAILED + nabídka rollbacku.
             NIKDY se mlčky netváří, že proběhla.
```

U T0 se kroky 1 a 4 zjednodušují (plán = jeden krok, ověření = jedno čtení), fáze 2 zůstává vždy.

### 17.5 Validace podle typu akce

| Akce | Třída | Nezávislá kontrola | Vratnost |
|---|---|---|---|
| Startup on/off | T0 | položka existuje? zápis přes StartupApproved? | vratné (znovu přepnout) |
| Přepínač soukromí | T0 | klíč existuje? hodnota v očekávaném rozsahu? | vratné (znovu přepnout) |
| Oprávnění app (Allow/Deny) | T0 | app existuje v ConsentStore? capability je známá? | vratné (znovu přepnout) |
| Driver opt-in | T0 | ovladač existuje? jde přes WUA? | vratné (odškrtnout) |
| Kill procesu | T1 | žije? `instance_id` sedí? třída ≠ critical/protected? | nevratné → potvrzení |
| Smazání souboru | T1 | existuje? kdo drží (Restart Manager)? cesta není systémová? | do koše = vratné |
| Uspání držitele | T1 | proces žije? není critical? má watchdog? | vratné (resume) |
| Odinstalace | T1 | uninstaller existuje? cesty patří aplikaci? | do koše + záloha registru |
| Delay-until-reboot delete | T1 | zápis do `PendingFileRenameOperations` | vratné do restartu |

Striktní režim (default zapnuto): každá **nevratná** T1 akce se navíc zazálohuje bodem obnovení (`SRSetRestorePoint`). T0 akce ho nepotřebují — jsou vratné z definice.

### 17.6 Audit — každá mutace nechává stopu

Každá akce, která projde vrstvou (schválená i zamítnutá), se zapíše do `audit` tabulky. Není to jen log, je to **součást bezpečnostního modelu**: uživatel (a ty při ladění) musí vždy vidět, co aplikace se systémem udělala a proč.

```sql
CREATE TABLE audit (
  id         INTEGER PRIMARY KEY,
  ts         INTEGER NOT NULL,
  action     TEXT NOT NULL,     -- kill|delete|startup_toggle|privacy_toggle|…
  target     TEXT NOT NULL,     -- co (pid+instance / cesta / klíč)
  class      TEXT NOT NULL,     -- 'T0' | 'T1'
  verdict    TEXT NOT NULL,     -- allow | deny
  deny_reason TEXT,             -- proč zamítnuto
  outcome    TEXT,              -- ok | failed | rolled_back | NULL(deny)
  reversible TEXT,              -- jak vrátit (recycle bin path, .reg záloha, …)
  detail     TEXT               -- JSON
);
CREATE INDEX ix_audit_ts ON audit(ts DESC);
```

Audit je trvalý (nemaže se retencí). Sloupec `reversible` drží konkrétní cestu zpět — odtud jde postavit „vrátit poslední akci" v UI.

---

## 18. Zámky, závislosti a bezpečné mazání

Vlajková funkce pro laika: *„proč to nejde smazat / ukončit a jak to bezpečně vyřešit."* Zároveň nejnebezpečnější modul — proto stojí celý na bezpečných primitivech a jeden vzor má explicitně zakázaný.

### 18.1 Kdo drží soubor — Restart Manager

Primární API: **Restart Manager** (`RmStartSession` → `RmRegisterResources` → `RmGetList`). Je to oficiální mechanismus, který používá i Windows Update. Vrátí pole `RM_PROCESS_INFO` s držiteli a **sám je klasifikuje** přes `RM_APP_TYPE`:

| `RM_APP_TYPE` | Význam | Naše třída |
|---|---|---|
| `RmCritical` | kritický systémový proces | **critical → akce zamčena** |
| `RmService` | Windows služba | service → čistá cesta (disable→delete→enable) |
| `RmMainWindow` / `RmOtherWindow` | app s oknem | user → graceful shutdown / kill |
| `RmConsole` | konzolový proces | user |
| `RmExplorer` | Explorer | zvláštní zacházení (nerestartovat naslepo) |

**Doplňkově pro identifikaci** (ne pro akci): když RM držitele nepokryje, `NtQuerySystemInformation(SystemExtendedHandleInformation)` + `NtQueryObject` → dohledá proces podle handle. Jen aby UI umělo říct „drží to tenhle proces". Akci vždy řídí bezpečná cesta níže.

### 18.2 Bezpečné mazání — postup

Viz vývojový diagram v odpovědi. Kroky:

1. **Zkus rovnou smazat** → `SHFileOperationW` s `FOF_ALLOWUNDO` (**do koše, vratné**). Když projde, hotovo.
2. **Zámek** → Restart Manager zjistí a klasifikuje držitele.
3. **Kritický držitel** → akce zamčena, konec. Uživateli se vysvětlí proč.
4. **Služba** → dočasně `SERVICE_DEMAND_START` + stop → smazat → obnovit původní konfiguraci.
5. **Uživatelský proces s watchdogem** → uspat rodiče (`NtSuspendProcess`) → ukončit držitele → smazat → probudit rodiče. **Podržení = uspání rodiče po dobu operace.** Křehké, běží v ms, s timeoutem a rollbackem.
6. **Drží to jádro / zamčeno od bootu** → `MoveFileEx(path, NULL, MOVEFILE_DELAY_UNTIL_REBOOT)` → smaže se při příštím startu. Zrušitelné editací `PendingFileRenameOperations` před restartem.
7. **Ověření** (fáze 4 validace) → soubor opravdu zmizel? Jinak FAILED + rollback.

### 18.3 Zakázaný vzor — nikdy neimplementovat

**Násilné zavření cizího handle** (`DuplicateHandle` cílového procesu + `DUPLICATE_CLOSE_SOURCE`). Vlastník o zavření neví, jeho příští zápis jde do neplatného handle → **pád nebo koruptovaná data**. Přesně to „něco rozbít", čemu se vyhýbáme. Tohle dělají „unlocker" nástroje a je to důvod, proč mají špatnou pověst. **Zakázáno.** Když jinak nelze, použij delay-until-reboot (krok 6) — pomalejší, ale bezpečné.

### 18.4 Trasování závislostí u kill procesu

Stejná logika pro ukončení procesu, na kterém závisí jiné:
- Před killem zjisti děti a závislé (strom z ETW parent PID).
- Preflight ukáže: *„Ukončení tohoto procesu ukončí i: … (3 závislé)."*
- Nabídni: (a) ukončit celý strom, (b) jen tento (děti osiří), (c) zrušit.
- Watchdog vzor: nabídni **podržet znovuspuštění** (uspat rodiče na dobu operace).

### 18.5 Crate

```
collector-lock/   # Restart Manager wrapper, handle scan (identifikace)
actor-file/       # bezpečné mazání, suspend/resume, delay-until-reboot,
                  # vše přes validační vrstvu z kap. 17
```

---

## 19. Fáze implementace

> **Pořadí a definice hotového jsou v `ROADMAP.md`** (v0–v11), aby existoval jediný zdroj pravdy a dokumenty si neprotiřečily. Tato sekce drží jen princip.

Řídící princip: **žádná mutace stavu systému před ověřeným validační vrstvou (v5).** Všechno čtení (procesy, historie, incidenty, inventář, Files-čtení, senzory, síť, bezpečnost) je bezpečné a přijde první. Mutace (startup, kill, odinstalace, mazání) až za bránou. Detaily, závislosti a brány kvality viz ROADMAP.

Mapování nových funkcí z tohoto kola:
- **GPU teploty + incidenty (pády/BSOD)** → k v3 (staví na ETW a detekci záseků, které tam vznikají).
- **CPU teploty (kaskáda) + FPS/frame time** → k v9 (čtecí sekce, `collector-sensors`).
- **Autologger jako povinný** → už od v3 (forenzní zdroj pro incidenty).

---

## 20. Rozpočet výkonu — kontrolní body

Po každé fázi změř. Když přesáhnete, zastavte se a optimalizujte, než půjdete dál.

| Metrika | Limit | Kde se to typicky rozbije |
|---|---|---|
| CPU (kolektor, idle systém) | < 0,5 % | ověřování podpisů v hot pathu; WMI; nefiltrovaný ETW FileIo |
| RAM (kolektor) | < 50 MB | neomezený ring buffer; cache podpisů bez limitu |
| Disk zápis | < 250 MB/den | chybějící retenční kaskáda |
| Latence zobrazení UI | < 150 ms | cold start (řeší pre-warm) |
| Latence samplu | < 5 ms | `EnumProcesses` místo `NtQuerySystemInformation` |
| Latence T0 akce (přepínač) | < 50 ms | těžká validace tam, kde stačí odlehčená (kap. 17.2) |
| Latence T1 validace | < 500 ms | pomalé skeny (WMI) v cestě validace |
| CPU validační vrstvy v klidu | **0 %** | vlákno na pozadí tam, kde má být čistě on-demand |

Poslední řádek je zásadní: validační vrstva v klidu **nesmí spotřebovat nic**. Když se nic nemění, `validate/` neběží — žádný poll, žádné vlákno. Přítomnost mutující cesty nesmí zdražit 24/7 běh démona.

---

## 21. Rizika, na která musíte být připraveni

1. **`NtQuerySystemInformation` je nedokumentované API.** Struktura se může mezi verzemi Windows měnit. Ověřuj velikost struktury za běhu, měj fallback na `EnumProcesses`.
2. **ETW vyžaduje admin + `SeSystemProfilePrivilege`.** Bez služby běžící jako SYSTEM to nejde.
3. **Nepodepsaná služba** bude hlásit SmartScreen a některé antiviry ji označí. Sežeňte code signing certifikát (EV je drahý, standardní OV stačí pro službu, ale SmartScreen reputaci budujete dlouho).
4. **Kill kritického procesu = BSOD.** Testujte allowlist ve VM. Nikdy ne na produkčním stroji.
5. **WebView2 pod zátěží.** Očekávejte, že plné UI při extrémním záseku nezareaguje — to je přijaté omezení, garanci drží démon, ne UI.
6. **MSIX kontejnery** mají virtualizovaný registr a filesystem — cesta, kterou aplikace vidí, není cesta na disku. `Package.EffectiveLocation` vs `InstalledLocation`.
7. **Suspend/resume rodiče je závod s časem.** Uspání watchdogu má timeout; když se operace nestihne, vždy resume a vrať se do bezpečného stavu. Nikdy nenech proces uspaný.
8. **`NtSuspendProcess` je nedokumentované.** Fallback: iterovat vlákna a `SuspendThread`. Vždy párovat resume, i při chybě uprostřed (RAII guard).
9. **TOCTOU (time-of-check to time-of-use) ve validaci.** Mezi validací (fáze 2) a provedením (fáze 3) uplyne čas — stav se může změnit. Minimalizuj okno: validuj těsně před provedením, ne s odstupem. U killu drž `instance_id`, u plánu `expires_ts`. Fáze 4 (ověření) je poslední pojistka, ale nenahrazuje krátké okno.
10. **Named pipe je útočná plocha.** Kdokoli s přístupem na pipe může posílat mutující požadavky. DACL omez na interaktivní `Users`, ověřuj token klienta u T1 akcí (`GetNamedPipeClientProcessId` → kontrola session), a **validuj vždy**, i kdyby požadavek vypadal důvěryhodně. UI není důvěryhodný zdroj pravdy.
11. **Oprávnění u Win32 aplikací nejsou tvrdě vynucená.** Windows není macOS — `ConsentStore` je pro nebalené aplikace z velké části poradní (aplikace může sáhnout na kameru přes DirectShow / ovladač a obejít to). **Největší riziko projektu není technické, ale komunikační:** kdyby UI budilo dojem zámku, uživatel by věřil v ochranu, kterou nemá. Barevné rozlišení vynuceno/nevynuceno (kap. 13.4) je proto **povinné**, ne kosmetické.
12. **ConsentStore není jediná cesta k hardwaru.** Aplikace bez záznamu v ConsentStore může kameru přesto používat. Neříkej „nikdo nepoužívá kameru" — říkej „žádná aplikace to Windows nehlásí". Rozdíl je zásadní.

---

## 22. Pravidla pro kód

- **Žádné OOP.** Data jsou `struct`, chování jsou volné funkce. Žádné traity jako náhrada dědičnosti.
- Moduly komunikují přes explicitní typy z `core-types`. Žádné sdílené mutable globály.
- Chyby: `Result` všude, `thiserror` pro typy chyb. **Každá chyba se propíše i do UI** — mlčky selhat je nepřijatelné.
- Stavy jsou izolované. Kolektor, který spadne, nesmí shodit službu — každý běží ve vlastním vlákně s watchdogem a restartem.
- **Vratnost jako default.** Každá mutující akce je vratná (koš místo hard delete, disable místo mazání, delay-until-reboot místo force). Nevratné akce jen za explicitním potvrzením a v striktním režimu s bodem obnovení.
- **Jediná cesta mutací.** Žádný `actor-*` ani jiný kód se nesmí dotknout systému bez `Verdict::Allow` z `validate/`. Neexistuje „rychlý obchvat" ani pro triviální akce — rychlé přepínače jdou stejnou vrstvou, jen v třídě T0 (kap. 17.2). Tuhle invariantu hlídej v code review.
- **`validate/` je izolovaná.** Nesmí záviset na `actor-*`, `collector-*`, `store`, `ipc` ani `ui`. Hlídej v CI (`cargo tree -p validate`). Ztráta izolace = ztráta nezávislosti validace.
- **Suspend vždy párovat s resume** přes RAII guard — i při chybě uprostřed operace se proces musí probudit. Nikdy nenech nic uspané.
- **Když si nejsme jistí, neděláme.** Validátor při pochybnosti akci zamítne. Falešné odmítnutí je přijatelné, poškození systému ne.
- **Nikdy se neskrývej.** Zákaz jakéhokoli kódu, který by nástroj vyloučil z vlastních výpisů (procesy, soubory, startup, spotřeba). Žádný `if pid == self { continue }`. Skrývání se je chování malwaru (kap. 2.3).
- **Nikdy nepředstírej záruku.** Kde OS něco nevynucuje, UI to musí přiznat — barvou i textem. Zelená znamená „vynuceno", ne „nastaveno" (kap. 13.4).
- Komentáře: nad každým blokem, česky nebo anglicky, konzistentně. Ne řádek po řádku.
- Vše konfigurovatelné (intervaly, retence, zapnuté kolektory) v jednom TOML, hot-reload.
