# Realizační roadmapa — systémový monitor pro Windows

> Doprovodný dokument k `SPEC.md`. Definuje **pořadí stavby** a **brány kvality**.
> Vůdčí princip: **žádná mutace stavu systému před ověřenou validační vrstvou (v5).**
> Sekundární princip: **od nejjednoduššího (jen čtení) po nejsložitější (mazání souborů).**

Každá verze je samostatně použitelná a samostatně testovatelná. Nezačínej další, dokud
aktuální nesplní svou **definici hotového (DoD)** a nepřejde svou **branou**.

Ke každé verzi: nejdřív si v Claude Code otevři odpovídající kapitoly SPEC, pak stav
jednu crate, pak k ní testy, pak měření. Teprve pak další.

---

## Přehled

| Verze | Téma | Riziko | Mění systém? |
|---|---|---|---|
| **v0** | Fundament a skelet | žádné | ne |
| **v1** | Živé procesy (jen čtení) | žádné | ne |
| **v2** | Identita aplikací | žádné | ne |
| **v3** | Historie a záseky (v Tasks) | žádné | ne |
| **v4** | Inventář, mapa souborů, disky (Files-čtení) | žádné | ne |
| **v5** | ⚠ BRÁNA: validační vrstva | — | připravuje mutace |
| **v6** | Startup položky | nízké (vratné) | ano, vratně |
| **v7** | Ukončování procesů | střední | ano |
| **v8** | Apps: odinstalace + Files: mazání se závislostmi | vysoké | ano, vratně |
| **v9** | Čtecí sekce: Hardware, Network, Security, Users | žádné | ne |
| **v10** | Drivers (detekce + opt-in update) | nízké (opt-in) | opt-in |
| **v11** | Dokončení a distribuce | — | — |

> Pořadí drží princip: všechno čtení je bezpečné a může přijít kdykoli, ale **mutace (v6–v8) smí až za bránou v5.** Čtecí sekce v9 jsou schválně až po mutacích jen proto, že jsou to „další obrazovky" bez závislostí — klidně se dají dělat dřív, pokud budete chtít prokládat. Jsou nezávislé na v5–v8.

---

## v0 — Fundament (žádné funkce, jen kostra)

**Cíl:** Prázdný, ale běžící a modulární skelet. Nic neumí, ale všechno má kam růst.

**Staví se:**
- Cargo workspace se všemi prázdnými crates dle SPEC kap. 2.2 (jen `lib.rs` se stuby).
- `core-types` — základní sdílené typy (zatím prázdné moduly, přidávají se průběžně).
- `svc` — binárka, která se **zaregistruje a běží jako Windows služba** (SCM handshake, start/stop/shutdown). Zatím jen loguje „žiju" jednou za sekundu.
- **Service Recovery watchdog** (SPEC kap. 2.3) — `sc failure` konfigurace, OS restartuje službu po pádu. Zdarma, žádný vlastní kód.
- **Kontrola integrity při startu** — služba ověří Authenticode podpis vlastních binárek; neshoda → nespustí se a nahlásí (SPEC kap. 2.3).
- `store` — SQLite: otevření, `PRAGMA` (WAL, synchronous=NORMAL), migrace schématu (zatím prázdné tabulky), retenční smyčka (běží, ale nemá co mazat).
- `ipc` — named pipe server + klient, echo request/response, postcard serializace, délkové rámce, DACL na pipe.
- `ui` — Tauri v2 + SvelteKit, prázdné okno, které se přes pipe zeptá služby „žiješ?" a zobrazí odpověď. Indikátor stavu démona v hlavičce (zeleně/červeně).
- Jeden `config.toml` + jeho načtení a hot-reload.

**Definice hotového:**
- `sc start syswatch` službu spustí, `sc stop` zastaví čistě.
- UI se otevře a zobrazí „služba běží" (přes pipe, ne napevno). Po `sc stop` zčervená.
- Služba přežije odhlášení/přihlášení uživatele (běží v session 0).
- **Zabití služby → OS ji sám restartuje** (Service Recovery funguje).
- Podvržená binárka → služba se odmítne spustit.
- Prázdná SQLite databáze se vytvoří na správném místě (`%ProgramData%\syswatch\`).

**Brána v0:** Restart PC → služba naběhne sama → UI se připojí. Zabij službu → naběhne zpět. Když tohle nejede, nestav nic dalšího.

**SPEC:** kap. 2 (vč. 2.3 sebeobrana), 8 (schéma), 10 (IPC).

---

## v1 — Živé procesy (první viditelná hodnota, jen čtení)

**Cíl:** Vidět živý seznam procesů. Zatím ploše, bez seskupení.

**Staví se:**
- `collector-proc` — sampler 1 Hz přes `NtQuerySystemInformation`. Buffer alokovaný jednou.
- Ring buffer v RAM (`VirtualLock`, bez alokací v hot pathu).
- Flusher → zápis vzorků do `sample_1s` a `system_1s`.
- `win-sys` — první safe wrappery (NtQuery*, priority, working set).
- IPC: `Subscribe{proc}` → UI dostává živý snapshot. `QuerySelfUsage` → vlastní spotřeba.
- UI: obrazovka **Tasks** jako plochá tabulka (PID, jméno, CPU, RAM, IO). Řazení, filtr.
- UI: obrazovka **Home** — živé grafy CPU/RAM/disk přes uPlot.
- UI: dlaždice **„Spotřeba nástroje"** v Settings (SPEC kap. 2.3) — vlastní CPU/RAM/zápis. **Rozpočet musí být ověřitelný uživatelem, ne slibovaný.**
- **Sebemonitoring:** nástroj se zobrazuje ve vlastním seznamu procesů jako každý jiný, se skutečnou spotřebou. **Žádný `if pid == self { continue }`.**

**Definice hotového:**
- Tabulka procesů se aktualizuje 1×/s, plynule.
- Grafy tečou.
- **Nástroj vidí sám sebe** ve vlastním výpisu, se stejnými čísly, jaká hlásí dlaždice spotřeby.
- **Změřeno:** kolektor < 0,5 % CPU, < 50 MB RAM na idle systému. Když ne → optimalizuj teď.

**Brána v1:** Nech to běžet 24 h. Zkontroluj, že RAM neroste (leak) a disk zápis sedí v rozpočtu po retenci. Ověř, že čísla v dlaždici spotřeby odpovídají tomu, co ukazuje Správce úloh.

**SPEC:** kap. 2.3 (sebemonitoring), 3.1, 3.4, 9.3 (grafy).

---

## v2 — Identita aplikací (seskupení procesů)

**Cíl:** `chrome.exe ×14` se sloučí pod jednu položku **Chrome**. Systémové procesy pod **Windows**.

**Staví se:**
- `identity` — rozhodovací kaskáda dle SPEC kap. 4.1 (override zatím prázdný, ale krok 0 existuje).
- Rozlišení `Microsoft Windows` vs. `Microsoft Corporation` (Edge/Office zůstávají samostatné).
- `sig_cache` — cache podpisů podle (cesta, velikost, mtime). Ověřování na `BELOW_NORMAL` vlákně, **nikdy v samplovacím cyklu.**
- Ochranné třídy procesů (critical/protected/system/user) — zatím jen pro **zobrazení** (žádné akce!).
- `identity_override` tabulka + IPC pro ruční přepis + jeho zobrazení (tečkovaný podtrh u `guess`).
- UI: obrazovka Procesy se změní na **strom aplikace → procesy**.

**Definice hotového:**
- Chrome, Explorer, VS Code atd. správně seskupené.
- Systémové procesy pod „Windows", ale Edge/Office/Teams samostatně.
- Ochranné třídy vizuálně odlišené (kritické šedě + zámek — zatím bez funkce).
- **Změřeno:** ověřování podpisů nezvedlo CPU nad rozpočet (cache funguje).

**Brána v2:** Projdi 20 náhodných aplikací očima — sedí seskupení? Kde ne, otestuj ruční override → napravé se a drží po restartu.

**SPEC:** kap. 4 celá.

---

## v3 — Historie, záseky a incidenty (killer feature #1, vč. pádů a BSOD)

**Cíl:** „Co mi před minutou zaseklo komp / co mi spadlo / proč byl BSOD." Časová osa s viníkem. Vše v Tasks (historie) + sekce Incidenty.

**Staví se:**
- Retenční kaskáda naostro: `sample_1s` → `10s` → `1m` (agregace avg+max).
- `collector-etw` — ETW session, providery Kernel-Process + Kernel-Disk + Kernel-Memory (hard faults). **Zatím bez Kernel-File** (ten je až pro mapu souborů, v4/v8).
- **Autologger `.etl` jako POVINNÝ** (SPEC kap. 3.2, 16.3) — forenzní černá skříňka, přežije BSOD.
- Přesný strom procesů z `ProcessStart` parent PID (nahrazuje odhad z PPID).
- Heartbeat vlákno (`TIME_CRITICAL`, 100 ms) + detekce záseku + přepnutí na 10 Hz burst.
- Klasifikace příčiny záseku (paging / IO / thermal / CPU / neznámé) dle SPEC kap. 3.3.
- **GPU teploty přes NVML/ADLX/IGCL** (SPEC kap. 15.2) — levné, do `sensor_1s`, hned součást časové osy.
- `collector-crash` — incidenty: pády aplikací (exit code z ETW + WER), hangy, BSOD (minidump + BugCheck po restartu). Sjednocený model (SPEC kap. 16).
- `event` + `incident` tabulky. UI: historie v Tasks + sekce **Incidenty** (detail komponentově dle kap. 16.4).

**Definice hotového:**
- Umělý zásek (zátěžový test: zaplav disk / vyžer RAM) se zaznamená a správně klasifikuje.
- Pád aplikace vytvoří incident s časovou osou; simulovaný BSOD (ve VM) se po restartu naparsuje z minidumpu + `.etl`.
- GPU teplota teče do grafů.
- **Změřeno:** ETW + autologger + NVML nezvedly CPU nad rozpočet; `.etl` rotuje a nepřerůstá.

**Brána v3:** Zásek vyvolaný diskem musí ukázat *disk* jako příčinu, ne CPU. BSOD ve VM musí po restartu vygenerovat incident s bugcheck kódem a daty z okna pádu (test, že autologger přežil).

**SPEC:** kap. 3.2, 3.3, 15.2 (GPU), 16 (incidenty), 8 (retence), 9.2.

---

## v4 — Inventář, mapa souborů, disky a Home (killer feature #2, stále jen čtení)

**Cíl:** „Co mám nainstalované a kde všude to má soubory." + přehled disků + Home dashboard. Bez učícího režimu a bez mazání (ta až v8).

**Staví se:**
- `collector-inv` — seznam aplikací z registry Uninstall + MSI + MSIX + (volitelně) winget.
- Mapa souborů: MSI file table (`Exact`), MSIX location (`Exact`), registry (`High`), heuristika (`Guess`).
- Klasifikace role cesty (install/config/data/cache/logs/registry) + velikosti (lazy, on-demand).
- `fs-index` (čtecí část) — NTFS MFT/USN prohlížeč, instant vyhledávání, barevné rozlišení souborů (SPEC kap. 11.2). **Bez mazání.**
- Přehled disků + SMART zdraví (SPEC kap. 11.1).
- Duplicity on-demand, dvoufázově (SPEC kap. 11.3) — čtecí analýza, mazání až v8.
- UI: obrazovka **Apps** (seznam + detail s mapou souborů + běžící procesy), obrazovka **Files** (disky + prohlížeč), obrazovka **Home** (grid dlaždic).
- Stav démona v hlavičce + badge zdraví na navigaci (SPEC kap. 9.2).
- Confidence vizuálně odlišené — `guess` cesty tečkovaně.

**Definice hotového:**
- U Chrome se ukáže instalace, User Data, Cache, registry větev — každá se štítkem zdroje.
- MSI aplikace mají přesný seznam souborů; portable jen heuristiku (a je to vidět).
- MFT prohlížeč najde soubor na celém svazku okamžitě; systémové/skryté barevně odlišené.
- SMART ukáže životnost disku. Home dashboard agreguje živá data.
- Skenování inventáře je on-demand nebo řídké, ne v cyklu — nezatěžuje.

**Brána v4:** Tady je přirozený **milník v1.0 k vydání.** Vše jen čte, nic nemůže rozbít. Kompletní, hodnotný, bezpečný nástroj (Home, Tasks, Apps, Files-čtení). Zvaž zveřejnění a sběr zpětné vazby, než se pustíš do mutací.

**SPEC:** kap. 5, 11 (čtecí části), 9.2.

---

## v5 — ⚠ BRÁNA: validační vrstva (nic viditelného, ale kritické)

**Cíl:** Postavit a **prověřit** nejdůležitější komponentu celého projektu dřív, než ji cokoli použije. Tahle verze nemá „funkci" pro uživatele. Má jistotu. Je to srdce bezpečnosti — všechny pozdější mutace (v6–v8, v10) jdou skrz ni.

**Staví se:**
- `validate/` jako **izolovaná crate** — závisí jen na `core-types` + `win-sys`, na ničem jiném (SPEC kap. 17.1). V CI ověř `cargo tree -p validate` (žádný `actor-*`/`collector-*`).
- Čtyřfázová kaskáda plán → validace → provedení → ověření (SPEC kap. 17.4), čtení **čerstvého stavu OS**, ne snapshotu z UI (17.3).
- **Dvě třídy akcí (17.2):** T0 (rychlá, vratná, < 50 ms, bez potvrzení — pro budoucí přepínače) a T1 (těžká, s preflightem a potvrzením). Obě jednou vrstvou.
- Rollback infrastruktura (RAII guardy pro suspend/resume, transakční kroky).
- Striktní režim + integrace `SRSetRestorePoint` pro nevratné T1.
- **Audit tabulka** (17.6) — každá akce (allow i deny) nechá stopu, se sloupcem `reversible`.
- IPC rozšíření: `Toggle*` (T0), `Plan*`/`Execute*` s `expires_ts` (T1).
- `actor-toggle` jako první triviální exekutor — no-op/testovací přepínač, na kterém se vrstva prověří.

**Definice hotového:**
- T1 akce projde všemi 4 fázemi a v UI ukáže plán → potvrzení → výsledek → ověření.
- T0 akce (testovací přepínač) proběhne pod 50 ms, bez dialogu, s auditem a možností undo.
- Uměle vyvolané selhání ve fázi 3 spustí rollback a označí akci FAILED (ne mlčky).
- Validátor odmítne akci na neexistujícím/kritickém cíli (test na fake i živých datech).
- **Změřeno:** `validate/` v klidu spotřebuje 0 % CPU (žádné vlákno na pozadí). Vrstva jde otestovat samostatně, bez zbytku aplikace.
- Expirovaný plán (`expires_ts`) je při `Execute` odmítnut.

**Brána v5:** Toto je **nejdůležitější brána projektu.** Dokud vrstva spolehlivě nezamítá špatné akce, neprovádí rollback a nezaznamenává audit, **nesmí vzniknout žádná skutečná mutace.** Otestuj cesty selhání víc než cesty úspěchu. Ověř izolaci crate.

**SPEC:** kap. 17 celá.

---

## v6 — Startup položky (první mutace, protože je vratná)

**Cíl:** Vidět a přepínat, co startuje s Windows. První mutace záměrně — je nejbezpečnější (plně vratná).

**Staví se:**
- `collector-boot` — 6 backendů čtení (Run klíče, složky, Task Scheduler, služby, MSIX, shell).
- Zápis přes `StartupApproved` (nedestruktivní, jak to dělá Správce úloh) — **ne mazání hodnot.**
- Vše přes validační vrstvu z v5 — startup přepínače jsou třída T0 (rychlé, vratné, bez potvrzení, SPEC kap. 17.2).
- Párování položek s aplikací (`app_id`) → „tohle spouští Adobe".
- Měření dopadu na boot (korelace přes ETW ProcessStart).
- UI: obrazovka **Po spuštění** — seskupené dle aplikace, přepínač, odhad dopadu.

**Definice hotového:**
- Vypnutí položky přežije restart; zapnutí ji vrátí. Ověřeno křížem se Správcem úloh.
- Žádná registry hodnota se nemaže — jen se přepíná stav.

**Brána v6:** Vypni a zapni 5 různých položek, restartuj, ověř stav. Nic se nesmí „ztratit".

**SPEC:** kap. 7, 17.

---

## v7 — Ukončování procesů (mutace střední závažnosti)

**Cíl:** Bezpečně ukončit proces, včetně trasování závislostí a preflightu.

**Staví se:**
- `actor-proc` — `PlanKill`/`Execute` přes validační vrstvu (`instance_id`, ne holý PID). Třída T1.
- Tvrdý allowlist: kritické procesy nelze (šedě), protected nelze (s důvodem), systémové za potvrzením.
- Trasování závislostí: preflight ukáže, kdo další padne (strom z ETW).
- Volby: ukončit strom / jen tento / zrušit.
- UI integrace do stromu procesů.

**Definice hotového:**
- Pokus o kill `csrss`/`wininit` je zablokovaný **před** jakýmkoli voláním (test ve VM!).
- Kill uživatelského procesu funguje; preflight správně předpoví závislé.
- Recyklace PID mezi zobrazením a klikem je ošetřená (`instance_id` mismatch → odmítnuto).

**Brána v7:** **Testuj výhradně ve virtuálce.** Ověř, že žádná cesta neumožní kill kritického procesu. Až pak na reálném stroji.

**SPEC:** kap. 4.3, 10, 17.

---

## v8 — Apps: odinstalace + Files: zámky a mazání (nejsložitější, proto naposled)

**Cíl:** Dvě vlajkové mutace pro laika — čisté odinstalování aplikace (bez zbytků) a odemknutí + smazání souboru, který něco drží. Nejriskantnější část, staví se poslední, s největší opatrností. Vše přes validační vrstvu z v5.

**Staví se:**
- `collector-lock` — Restart Manager (identifikace + klasifikace držitelů) + handle scan (jen identifikace).
- `actor-file` — bezpečné mazání dle SPEC kap. 18.2:
  - koš (`FOF_ALLOWUNDO`) jako default,
  - služba: disable → delete → restore,
  - watchdog: suspend rodiče → kill → delete → resume (timeout + RAII resume),
  - neřešitelné: `MOVEFILE_DELAY_UNTIL_REBOOT`.
- `actor-app` — odinstalace (SPEC kap. 5.3): oficiální uninstaller → úklid zbytků do koše, evidence „zbytků"; asistované vratné odstranění (5.4) když uninstaller není. **Nikdy force delete.**
- `fs-index` (mazací část) — duplicity → smazání do koše.
- **Zakázaný vzor** (`DUPLICATE_CLOSE_SOURCE`) se neimplementuje — pojistka v code review.
- Volitelný učící režim mapy souborů: ETW Kernel-File s filtrem (`confidence=observed`).
- UI: Files — „proč to nejde smazat" + bezpečné řešení; Apps — odinstalace + revidovatelný strom zbytků.

**Definice hotového:**
- Smazání zamčeného souboru (drženého uživatelskou app) projde přes ukončení držitele → koš → ověření.
- Odinstalace s oficiálním uninstallerem uklidí zbytky do koše; bez něj asistované vratné odstranění.
- Kritický držitel akci zablokuje s vysvětlením.
- Suspend/resume nikdy nenechá proces uspaný (i při chybě uprostřed — ověřeno testem selhání).
- Vše vratné: soubory z koše, registr ze zálohy.

**Brána v8:** Nejpřísnější testování celého projektu, výhradně ve VM nejdřív. Projdi každou cestu selhání: co když kill selže, co když se soubor mezitím uvolní, co když rodič nejde uspat, co když uninstaller spadne. Každá cesta musí končit v bezpečném, konzistentním stavu.

**SPEC:** kap. 18 celá, 17, 5.3–5.4, 11.3–11.4.

---

## v9 — Čtecí sekce: Hardware, Network, Security, Users

**Cíl:** Doplnit levné čtecí sekce, které dělají z nástroje švýcarský nůž. Všechno jen čte. Přepínače soukromí (Allow/Deny jako T0 akce) se ZRUŠILY — viz rozhodnutí na konci sekce. Celá v9 je tím nezávislá na v5–v8 a dala se dělat i dřív.

**Staví se:**
- `collector-hw` — inventář hardwaru + SMART + baterie (SPEC kap. 15.1).
- `collector-sensors` — **CPU teploty** degradační kaskádou (HWiNFO/LHM → ACPI → throttling+takty, SPEC kap. 15.2) + **FPS/frame time** přes ETW DXGI/Dwm, opt-in per proces (SPEC kap. 15.3). GPU teploty už jsou z v3.
- Komponentově orientované karty (SPEC kap. 15.4): graf nahoře, údaje pod ním, historie v kartě.
- `collector-net` — spojení per aplikace, porty, trafik v čase, geo (offline), WiFi, signály (SPEC kap. 12). **Bez DPI.**
- `collector-sec` — stav ochrany, signály procesů, telemetrie (čtení) + **oprávnění aplikací přes CapabilityAccessManager ConsentStore** (SPEC kap. 13.4): kdo má přístup, **kdo právě používá** (`LastUsedTimeStop == 0`), historie použití. Čteno **událostně** (`RegNotifyChangeKeyValue`), ne pollem. Přepínače Allow/Deny se NESTAVÍ (rozhodnutí na konci sekce).
- `collector-users` — účty, oprávnění, historie přihlášení (SPEC kap. 14).
- UI: obrazovky Hardware (komponentově), Network, Security, Users.

**Definice hotového:**
- Hardware odpovídá Správci zařízení + SMART ukazuje životnost disků.
- GPU teplota vždy; CPU teplota když ji zařízení hlásí, jinak throttling+takty se zdrojem „nedostupné".
- FPS/frame time měřené u hry bez injektáže; spiky se napojí na záseky.
- Network mapuje spojení na aplikace, ukazuje kam a kolik.
- Security ukáže stav ochrany na jedné obrazovce.
- **Oprávnění:** živá tečka u aplikace, která právě používá kameru/mikrofon. Historie: *„Discord používal mikrofon včera 3 h 12 min."*
- **Vynucení je barevně rozlišené:** MSIX = zeleně „zablokováno" (Windows vynutí), Win32 = jantarově „odepřeno, ale nevynuceno". **Zelená nikdy tam, kde vynucení není.**
- Users ukáže, kdo má admin práva.
- Všechny sekce v rozpočtu, žádné WMI zatuhnutí, žádný kernel driver.

**Rozhodnutí (18. 8. 2026): přepínače soukromí se nestaví.**

Nástroj se během vývoje posunul k tomu, aby o systému **vypovídal**, ne
aby ho ovládal. Přepínač Allow/Deny u oprávnění by navíc u klasických
aplikací stejně nic nevynutil — Windows ho tvrdě vymáhají jen
u balených, což sekce sama přiznává jantarovou barvou. Nabízet vypínač,
který v půlce případů nevypíná, je horší než ho nemít.

Co tím padá: `actor-consent`, T0 akce nad ConsentStore a závislost v9
na validační vrstvě z v5. Co zůstává: čtení oprávnění, živá tečka
u aplikace, která právě používá kameru nebo mikrofon, a historie
použití. Existující ovládací prvky (odinstalace, ukončení procesu,
přepínače po spuštění) se tím neruší — rozhodnutí se týká toho, co se
nově přidává.

**Brána v9:** Každá sekce v rozpočtu na Win10 i Win11. Žádný verdikt tam, kde má být signál (Security, Network). CPU teplota nikdy nepředstírá číslo, které nemá (vždy uveď zdroj). **U oprávnění nikdy nepředstírej tvrdý zámek u Win32 aplikací** — to je nejdůležitější kontrola této fáze, protože falešný pocit ochrany je horší než žádný.

**SPEC:** kap. 12, 13, 14, 15.

---

## v10 — Drivers (detekce + opt-in aktualizace)

**Cíl:** Přehled ovladačů a verzí + upozornění na aktualizace. Instalace jen na opt-in u konkrétního ovladače.

**Staví se:**
- `collector-drv` — SetupAPI inventář (`SetupDiEnumDeviceInfo`, **ne WMI**).
- Kontrola aktualizací přes WUA (`Type='Driver'`) — detekce a notifikace.
- Skenování při startu + na `WM_DEVICECHANGE`, nikdy v cyklu.
- **Opt-in instalace per ovladač** (checkbox), přes WUA, s povinným bodem obnovení. Přes validační vrstvu z v5.
- UI: obrazovka **Drivers** — zařízení, verze, datum, podpis, dostupná aktualizace, checkbox auto-update.

**Definice hotového:**
- Seznam ovladačů odpovídá Správci zařízení.
- Bez zaškrtnutí se nic neinstaluje; se zaškrtnutím jen ten jeden, s bodem obnovení předem.

**Brána v10:** Ověř, že inventář neblokuje a je rychlý. Otestuj, že bez opt-inu se opravdu nic nenainstaluje.

**SPEC:** kap. 6 celá.

---

## v11 — Dokončení a distribuce

**Cíl:** Z funkčního nástroje udělat nástroj, který jde nainstalovat a používat dennodenně.

**Staví se:**
- Globální klávesová zkratka + pre-warm UI (skryté od startu, zkratka jen `ShowWindow`).
- Priority a working set lock UI procesu (aby zkratka fungovala i pod zátěží — v mezích WebView2).
- **Obrazovka prvního spuštění** (SPEC kap. 9.5) — kritické nastavení, každá položka odklikaná: služba při startu (kritické!), UI při přihlášení, zkratka, striktní režim, které kolektory. **Nic se nezapne mlčky** — nástroj hlídající soukromí musí být sám vzorem.
- Instalátor WiX MSI (registrace služby, autostart, oprávnění) — `INFRA.md` kap. 4.5.
- Auto-updater (Tauri) + aktualizace služby přes MSI + verzní kontrakt IPC — `INFRA.md` kap. 4.3.
- **Code signing** služby i UI (SmartScreen, antiviry) — `INFRA.md` kap. 4.4.
- Finální průchod výkonového rozpočtu na reálném stroji, 7 dní.
- Dokumentace pro laika.

**Definice hotového:**
- Čistá instalace na cizím PC funguje bez ručních zásahů.
- **První spuštění se zeptá na vše kritické; nic není zapnuté bez potvrzení.** Odmítnutí autostartu je respektováno (služba jde na `DEMAND_START`).
- Nástroj se zobrazuje ve vlastní sekci Start jako běžná položka — viditelný, vypnutelný, s varováním.
- Zkratka vyvolá UI < 150 ms na běžně zatíženém systému.
- Aktualizace služby i UI proběhne bez pádu (verzní kontrakt drží).
- Podpis platí, SmartScreen nekřičí (nebo je to zdokumentované).

**Brána v11:** Nainstaluj na stroj, který není tvůj vývojový, a nech tam běžet týden.

---

## Zlatá pravidla pro celou cestu

1. **Nepřeskakuj brány.** Každá existuje, protože další verze na ní staví.
2. **Měř po každé verzi.** Výkonový rozpočet je podmínka, ne cíl. Regrese se ladí, když je malá.
3. **Mutace testuj ve VM první.** v6–v8 mají cesty, které při chybě rozbijí systém. VM je levná.
4. **Když si validátor není jistý, akci zamítni.** Falešné odmítnutí je otrava; poškození je konec důvěry.
5. **v4 je přirozený release.** Zvaž vydání „read-only" verze a sběr zpětné vazby, než se pustíš do mutací.
6. **Vratnost je default.** Koš, ne hard delete. Disable, ne mazání. Delay-until-reboot, ne force.
7. **Nikdy nepředstírej záruku, kterou nemáš.** Zelená = „vynuceno", ne „nastaveno". Falešný pocit ochrany je horší než žádný — u oprávnění (v9) to platí dvojnásob.
8. **Nikdy se neskrývej.** Nástroj se zobrazuje ve vlastních výpisech se skutečnou spotřebou. Rozpočet musí být ověřitelný, ne slibovaný. Skrývání se je chování malwaru.
9. **Nic se nezapíná mlčky.** Nástroj hlídající soukromí musí být sám vzorem — kritické nastavení se odklikává (v11).
