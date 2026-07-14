# Design zásady — syswatch UI

> Závazný dokument pro veškeré UI. Vychází ze SPEC kap. 9 (9.2 globální
> prvky, 9.3 grafy, 9.4 vizuální styl) a kap. 13.4 (barevné kódování
> vynucení). Upřesnění majitele projektu: **akcent je čistě bílá**,
> barvy se používají **cíleně a účelně podle funkce**, lehká inspirace
> nativním Windows 11. Tokeny žijí v `crates/ui/src/app.css` — kód
> nikdy nepoužívá barvy natvrdo, vždy přes CSS proměnné.

---

## 1. Charakter

Tech + minimalismus. Nástroj, ne hračka: hustota informací je hodnota,
ozdoba je dluh. UI je tmavé, klidné a tiché — **pozornost si smí říct
jen skutečný stav systému** (červená tečka, jantarové varování, badge),
nikdy dekorace.

Inspirace Windows 11 znamená: vrstvené tmavé povrchy (mica/card pocit),
decentní 1px bordery místo stínů, zaoblení 4–8 px, systémová typografie
jako fallback, žádné ostré kontrasty mimo funkční barvy.

## 2. Barvy

### 2.1 Základ (neutrály)

| Token | Hodnota | Použití |
|---|---|---|
| `--bg` | `#0f0f12` | pozadí aplikace |
| `--surface` | `rgba(255,255,255,.035)` | karty, hlavička, panely |
| `--surface-hover` | `rgba(255,255,255,.06)` | hover řádků/karet |
| `--border` | `rgba(255,255,255,.08)` | standardní 1px border |
| `--border-strong` | `rgba(255,255,255,.14)` | fokus, aktivní ohraničení |
| `--text` | `#ececef` | primární text |
| `--text-dim` | `#9a9aa1` | sekundární text, popisky |
| `--text-faint` | `#5c5c63` | terciární text, hinty |

Vrstvení = průhledná bílá nad `--bg`, ne nové odstíny šedé. Hlubší
vrstva → nižší alfa. Tím vzniká konzistentní hloubka jako ve Win11.

### 2.2 Akcent — čistě bílá

`--accent: #ffffff`. Používá se **střídmě**: název aplikace, aktivní
položka navigace, primární hodnota v kartě, aktivní stav přepínače.
Akcent je důraz, ne barva „značky" rozlitá po ploše. Pravidlo: na jedné
obrazovce má bílý akcent svítit na jednotkách míst, ne desítkách.

### 2.3 Funkční barvy — výhradně podle významu

| Token | Hodnota | Význam — a NIC jiného |
|---|---|---|
| `--ok` | `#5dbb63` | **běží / chráněno-vynuceno / zdravé**. Zelená = „OS to vynucuje / stav je ověřen", nikdy „nastaveno" (SPEC 13.4) |
| `--danger` | `#e5484d` | **neběží / kritické / destruktivní akce** |
| `--warn` | `#f0b429` | **varování / nevynuceno / degradováno** — typicky „odepřeno, ale Windows to u Win32 nevynucuje", systémové procesy |

Tvrdá pravidla:
- Barva nikdy není dekorace. Když prvek nenese stav, je neutrální.
- **Zelená nikdy tam, kde vynucení není** (SPEC 13.4) — falešný pocit
  ochrany je horší než žádný.
- Významová barva jde vždy s textem/ikonou, nikdy sama (přístupnost,
  barvoslepost): tečka + „služba běží", ne jen tečka.
- Ochranné třídy procesů (SPEC 9.4): kritické = šedé + zámek (nikoli
  červené — nejsou „špatné", jsou nedotknutelné), systémové = jantarová,
  uživatelské = neutrální.

## 3. Typografie

| Role | Font | Poznámka |
|---|---|---|
| Nadpisy (h1–h3, názvy karet) | `Tiempos Headline`, fallback Georgia/serif | serif dává nástroji tvář; než koupíme font, nese to fallback |
| Text, tabulky, UI | `Inter`, fallback `Segoe UI Variable Text`, `Segoe UI` | Segoe fallback = přirozený Win11 pocit |
| Čísla v tabulkách/grafech | Inter s `font-variant-numeric: tabular-nums` | sloupce čísel se nesmí klepat |

Základ 14 px / 1.45. Stupnice střídmá: 0.78 / 0.82 / 0.95 / 1.1 rem.
Hierarchii tvoří primárně **barva textu** (text → dim → faint) a váha,
ne velikost.

## 4. Geometrie a prostor

- Radius: 4 px (malé prvky), 6 px (výchozí), 8 px (karty, dialogy).
- Odsazení v násobcích 4 px; uvnitř karet 12–20 px.
- Bordery 1px `--border`; stíny nepoužíváme (tmavý režim je zabíjí),
  hloubku dělá vrstvení povrchů.
- Glassmorphism (`backdrop-filter: blur`) jen na **překryvných** prvcích:
  hlavička, případné dialogy. Ne na statických kartách — je to drahé
  (WebView2) a bez pohybu pod sklem nemá smysl.

## 5. Globální prvky (SPEC 9.2)

- **Indikátor démona** v hlavičce, na každé obrazovce: tečka `--ok` /
  `--danger` + text „služba běží / neběží". Zdroj pravdy je vždy pipe,
  nikdy domněnka UI.
- **Badge zdraví** na položkách navigace: tečka `--warn`/`--danger`
  u sekce, která volá po pozornosti. Jediný povolený „křiklavý" prvek
  navigace.

## 6. Komponentový princip (SPEC 15.4)

Vše o jedné entitě je pohromadě u ní: **nahoře graf (živý i historie
přes týž přepínač času), pod ním údaje**. Historie není samostatná
obrazovka. Platí pro hardware, procesy, aplikace i incidenty.

## 7. Grafy (SPEC 9.3)

- uPlot, nikdy Chart.js.
- Křivky: neutrální bílá/šedá pro primární metriku; funkční barvy jen
  když křivka nese význam (teplota v throttlingu = `--warn`).
- Bez výplňových gradientů „pro krásu"; max jemná (≤ 8 % alfa) výplň
  pod primární křivkou.
- Osy a mřížka: `--text-faint` / `--border`, nesmí konkurovat datům.

## 8. Stavové vzory

| Stav | Vzor |
|---|---|
| Načítám | skeleton/`--text-faint` text, žádné spinnery přes celou plochu |
| Prázdno | tichý text `--text-dim` s vysvětlením, co tu bude |
| Chyba | text `--danger` + co se stalo + co s tím; chyby se nikdy nepolykají (SPEC 22) |
| Neznámá hodnota | pomlčka + zdroj („—, senzor nedostupný"), nikdy vymyšlené číslo (SPEC 15.2) |
| Confidence `guess` | tečkovaný podtrh (SPEC 4.4, 5.2) |

## 9. Ikony a pohyb

- Ikony: **Lucide** (outline), 16/20 px, barva dle textu, ne vlastní sady.
- Animace svižné a účelné: 120–160 ms ease-out na hover/rozbalení;
  žádné vjezdy obrazovek. `prefers-reduced-motion` respektovat.
- SPA — žádné přenačítání (SPEC 9.4).

## 10. Do / Don't

**Do:** tmavý klid, data hustě a čitelně, barva = informace, bílý akcent
vzácně, tabular-nums, vrstvení průhlednou bílou.

**Don't:** barevné dekorace, zelená bez vynucení, červená pro „jiné než
problém", stíny, gradienty, spinnery přes obsah, více než jedna křiklavá
věc na obrazovce, hardcoded barvy mimo tokeny.
