# Design zásady — Winsent UI

> Závazný dokument pro veškeré UI. **Hlavní návrh rozložení je „Frame 5"**
> (mock od majitele projektu) — titlebar se stavem, sidebar s Lucide
> ikonami, obsahový panel. Ostatní reference (PRE-DATA dashboard,
> industrial mapa, Smart Home) jsou inspirace pro náladu: glow u barev,
> industrial dotted + frame prvky, čisté karty. Doplňuje SPEC kap. 9
> (9.2 globální prvky, 9.3 grafy) a 13.4 (barevné kódování vynucení).
> Tokeny žijí v `crates/ui/src/app.css` — žádné barvy natvrdo v kódu.

---

## 1. Charakter

**Tech-industriální, ale clean.** Tmavý režim (jediný). Minimalismus
s hustotou informací; ozdoba je dluh. Lehký **glow** u významových barev
(tečky stavu, křivky grafů) — světlo, ne lesk. **Žádné glossy** — nikdy
plastické odlesky, gumové bublinové tlačítka ani heavy blur.

Windows 11 pocit: vrstvené tmavé povrchy, decentní 1px bordery,
radius 6–10 px, klid.

Industrial vrstva (střídmě, jako koření): tečkované (dotted) linky
a kroužky, tenké rámové linky s "ticks", technické popisky v mono
uppercase (`FIRA MONO 10–11 px, letter-spacing 0.08em`), číslované
sekce. Vždy podřízené čitelnosti.

## 2. Gradienty — tvrdé pravidlo

- **Pozadí prvků (karty, panely, tlačítka): NIKDY gradient.** Plné
  poloprůhledné povrchy.
- Gradient smí být jen: (1) na **úplně zadním pozadí okna** (velmi
  jemný, tmavý radiál), (2) **v barvách samotných** — tahy křivek
  v grafu, barevné přechody indikátorů (např. čára CPU přecházející
  z bílé do jantarové při throttlingu).

## 3. Barvy

### 3.1 Neutrály (vrstvení průhlednou bílou nad --bg)

| Token | Hodnota | Použití |
|---|---|---|
| `--bg` | `#0e0f12` | zadní pozadí okna (+ jemný radiál) |
| `--panel` | `#16171c` | sidebar, hlavní obsahový panel (Frame 5) |
| `--surface` | `rgba(255,255,255,.04)` | karty uvnitř panelu, hover |
| `--surface-hover` | `rgba(255,255,255,.07)` | hover řádků, aktivní nav |
| `--border` | `rgba(255,255,255,.08)` | standardní 1px border |
| `--border-strong` | `rgba(255,255,255,.16)` | fokus, aktivní ohraničení |
| `--text` | `#ececef` / `--text-dim` `#9a9aa1` / `--text-faint` `#5c5c63` | hierarchie textu |

### 3.2 Akcent — čistě bílá

`--accent: #ffffff`. Wordmark, aktivní položka navigace, primární
hodnoty, primární křivka grafu. Střídmě — jednotky výskytů na obrazovku.

### 3.3 Funkční barvy + glow

| Token | Hodnota | Význam — a NIC jiného |
|---|---|---|
| `--ok` | `#4ade80` | běží / vynuceno / zdravé (zelená NIKDY bez vynucení, SPEC 13.4) |
| `--danger` | `#ef4444` | neběží / kritické / destruktivní |
| `--warn` | `#f59e0b` | varování / nevynuceno / degradace |

Glow = `box-shadow: 0 0 8px color-mix(in srgb, var(--barva) 55%, transparent)`
u teček a malých prvků; křivky v grafu mají jemný stín stejné barvy.
Glow je vyhrazen významovým barvám a akcentu — šedé prvky nesvítí.
Barva vždy s textem/ikonou, nikdy samotná (barvoslepost).

## 4. Typografie

| Role | Font |
|---|---|
| UI, nadpisy, navigace | **Space Grotesk** (400/500/600) |
| Čísla, tabulky dat, technické popisky, kód | **Fira Mono** (400/500) |

Oba fonty bundlované lokálně (@fontsource), žádné CDN. Základ 14 px /
1.45. Datové hodnoty vždy Fira Mono (tabular už z podstaty). Technické
popisky: Fira Mono, uppercase, 10–11 px, `letter-spacing: .08em`,
`--text-faint`.

## 5. Rozložení — Frame 5 (závazné)

```
┌ titlebar ──────────────────────────────────────────────┐
│ ⛨ Winsent      ● stav služby            —  □  ✕        │
├──────────┬─────────────────────────────────────────────┤
│ sidebar  │  obsahový panel (radius 10, border, --panel) │
│ (panel)  │                                              │
└──────────┴─────────────────────────────────────────────┘
```

- **Titlebar**: vlastní (bez systémových dekorací), drag region.
  Vlevo logo (Lucide `shield`) + wordmark **Winsent**. Vedle stav
  démona: tečka s glow + text (zelená „služba běží" / červená
  „služba neběží"). Vpravo okenní tlačítka — minimalizace, maximalizace,
  zavřít; ✕ má hover/akcent červený (Frame 5).
- **Sidebar**: samostatný zaoblený panel. Položky = Lucide ikona
  (20 px, stroke 1.75) + text. Aktivní: bílý text + `--surface-hover`
  pozadí; neaktivní `--text-dim`. Settings ukotvené dole.
- **Sekce navigace** (názvy dle Frame 5): Home, Tasks, Programs, Files,
  On start, Users, Hardware, Drivers, Connection, Network, Security
  + dole Settings. (Badge zdraví na položkách dle SPEC 9.2 přijde
  s daty.)
- **Obsahový panel**: jeden velký zaoblený rect s borderem; obsah
  scrolluje uvnitř něj.

## 6. Obrazovky — priority

- **Tasks je hlavní obrazovka**: nahoře hlavní časový graf (živý,
  později s přepínačem historie — komponentový princip SPEC 15.4),
  pod ním tabulka procesů. `/` přesměruje na Tasks.
- **Home je jen souhrn — odloženo**, neřešit, dokud nejsou data ze
  sekcí.
- Prázdné sekce: tichý placeholder (mono popisek + kdy přijde obsah).

## 7. Grafy (uPlot)

- Primární křivka bílá (akcent), sekundární `--text-dim`; významová
  barva jen když křivka nese význam. Lehký glow tahu.
- Gradient povolen v tahu křivky (barva→barva), ne ve výplni pozadí;
  výplň pod křivkou max 6–8 % alfa, jednobarevná.
- Osy/mřížka: `--text-faint`/`--border`, tečkovaná mřížka (industrial),
  popisky os Fira Mono 10 px.
- Nikdy Chart.js.

## 8. Stavové vzory

| Stav | Vzor |
|---|---|
| Načítám | skeleton / mono „—" , žádné celoplošné spinnery |
| Prázdno | mono popisek `--text-faint`: co tu bude a od kdy |
| Chyba | `--danger` text + co se stalo + co s tím; nikdy mlčky (SPEC 22) |
| Neznámá hodnota | „—" + zdroj; nikdy vymyšlené číslo (SPEC 15.2) |
| Confidence `guess` | tečkovaný podtrh (SPEC 4.4) |

## 9. Ikony a pohyb

- **Lucide** outline, přesně sada z Frame 5 pro navigaci; 16–20 px.
- Animace 120–160 ms ease-out (hover, rozbalení); žádné vjezdy
  obrazovek; `prefers-reduced-motion` respektovat. SPA bez přenačítání.

## 10. Do / Don't

**Do:** tmavý klid, glow jen na významu, dotted/frame detaily střídmě,
Fira Mono na číslech, vrstvení průhlednou bílou, Frame 5 rozložení.

**Don't:** glossy/plastické povrchy, gradienty na pozadích prvků,
zelená bez vynucení, dekorativní barvy, stíny místo borderů, více než
jedna křiklavá věc na obrazovce, hardcoded barvy mimo tokeny.
