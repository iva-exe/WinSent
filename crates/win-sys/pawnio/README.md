# PawnIO moduly

Dva moduly pro ovladač [PawnIO](https://github.com/namazso/PawnIO), přes které
služba Winsent čte teplotu CPU (`crates/win-sys/src/pawnio.rs`). Vkládají se do
`syswatch.exe` přes `include_bytes!`. Samotný ovladač Winsent nešíří: uživatel si
ho instaluje sám z <https://pawnio.eu> nebo přes `winget install namazso.PawnIO`.

## Původ (ověřeno 10. 10. 2026)

- Nezměněné oficiální podepsané sestavení, vydání **0.2.11**:
  <https://github.com/namazso/PawnIO.Modules/releases/tag/0.2.11>.
  Tag 0.2.11 ukazuje na commit `52a7e536dff3e53c96917a28caac5e0fa6510696`. Tag je podepsaný
  autorem a GitHub ho hlásí jako ověřený.
- `release_0_2_11.zip` má SHA-256 `43608cb89bc84247fef1368a139013f7d043e17db6d6c8dfc9b46bf0905a81f4`.
  Shoduje se s otiskem, který GitHub u vydání zveřejňuje.
- Podpis modulů je RSA-4096, PKCS#1 v1.5, SHA-256 nad obrazem AMX. Ověřen proti jedinému
  důvěryhodnému klíči ovladače (`k_pubkey_namazso_2023` ve zdrojích PawnIO) třemi nezávislými
  způsoby: Node/OpenSSL, ruční RSA a Windows CNG.
- Soubory jsou bajtově shodné s kopiemi v LibreHardwareMonitoru
  (`LibreHardwareMonitorLib/Resources/PawnIo/`).

| Soubor | Velikost | SHA-256 |
|---|---|---|
| `AMDFamily17.bin` | 10 652 B | `dae74615761b78bdf064dfb3e136252ddcc6fc727d88f14738d0e5800d427a91` |
| `IntelMSR.bin` | 5 324 B | `d6ed85d65ab17a22f813ef98207d6d537155ee2ded5976a21cb48413c9b92e5f` |

## Co se z nich volá

Jen čtení s pevnou adresou:
- `ioctl_read_smn(0x59800)`: teplota AMD Zen.
- `ioctl_read_msr(0x1A2 / 0x1B1)`: TjMax a teplota pouzdra Intel.

Zápisová funkce `ioctl_write_msr`, kterou moduly také obsahují, se nikdy nevolá.
`pawnio.rs` je postavený tak, aby to ani nešlo:
- jména funkcí a adresy jsou uzavřený výčet;
- moduly se načítají jen po kontrole SHA-256;
- veřejné API nebere žádný parametr pro ovladač.

Jediný zápis uvnitř čtení: `ioctl_read_smn` nejdřív zapíše adresu do indexového
registru SMN (PCI 0:0.0, offset 0x60) a pak přečte data z 0x64. Index jen vybírá
čtený registr a žádné nastavení procesoru nemění; stejně to dělá Linux k10temp.
Dvojici index/data sdílejí všechny nástroje, proto Winsent čte jen pod mutexem
`Global\Access_PCI`. Když mutex nemá, čtení vynechá.

## Licence

```
Third-party component: PawnIO Modules — AMDFamily17.bin, IntelMSR.bin
Unmodified official signed builds, release 0.2.11:
  https://github.com/namazso/PawnIO.Modules/releases/tag/0.2.11
Copyright (C) 2025 namazso <admin@namazso.eu> and contributors
License: GNU Lesser General Public License v2.1 or (at your option) any later
version (SPDX: LGPL-2.1-or-later); full text in COPYING next to this file.

This library is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
Lesser General Public License for more details.

Corresponding source: src/ next to this file (AMDFamily17.p, IntelMSR.p and
the include files they use, from tag 0.2.11), also
  https://github.com/namazso/PawnIO.Modules/tree/0.2.11
Build: pawncc <module>.p -iinclude -C64 -;+ -(+ -p  (Pawn 4.1.7152, per the
upstream CI), then signed by the author; only signed builds load in the
official driver.
```
