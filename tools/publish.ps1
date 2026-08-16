# Vydání nové verze pro testery — jeden příkaz.
#
#   powershell -ExecutionPolicy Bypass -File tools\publish.ps1
#
# Postaví release binárky, položí je do release/ (odkud si je stahuje
# WinsentSetup.exe) a pushne do repozitáře. Testerům stačí spustit
# instalátor znovu a mají aktuální verzi.
#
# POZOR na kódování: soubor musí zůstat v UTF-8 s BOM (PowerShell 5.1).

#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# Verze = číslo z Cargo.toml + datum a čas buildu. Instalátor podle
# ní pozná, že má co stahovat — samotné 0.1.0 by se během testování
# neměnilo a aktualizace by se přeskakovala.
$cargo = Get-Content (Join-Path $root 'Cargo.toml')
$inPkg = $false
$base = $null
foreach ($line in $cargo) {
    if ($line -match '^\s*\[workspace\.package\]') { $inPkg = $true; continue }
    if ($inPkg -and $line -match '^\s*\[') { break }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $base = $Matches[1]; break }
}
if (-not $base) { throw "verzi se nepodařilo přečíst z Cargo.toml" }
$version = "$base+$(Get-Date -Format 'yyyyMMdd.HHmm')"

Write-Host "Winsent — vydávám $version" -ForegroundColor Cyan

# ── Build ──────────────────────────────────────────────────────────
# UI se MUSÍ stavět přes Tauri CLI, ne přes holé `cargo build`.
# `tauri::generate_context!` se podle prostředí rozhoduje, jestli do
# binárky vestaví soubory frontendu, nebo jen adresu vývojového
# serveru. Bez CLI vyhraje ta druhá možnost a nainstalovaná aplikace
# ukáže „localhost se odmítl připojit" — vypadá to jako rozbitá
# aplikace, přitom jde jen o špatně postavenou binárku.
# CLI si samo pustí `beforeBuildCommand` (vite build), takže se
# frontend staví v rámci tohohle kroku.
Write-Host "1/4  Aplikace (Tauri build)"
Push-Location (Join-Path $root 'crates\ui')
& .\node_modules\.bin\tauri.exe build --no-bundle
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) { throw "tauri build selhal ($code)" }

Write-Host "2/4  Služba a instalátor (release)"
& cargo build --release -p svc -p installer
if ($LASTEXITCODE -ne 0) { throw "cargo build selhal ($LASTEXITCODE)" }

# Kontrola, že v binárce OPRAVDU jsou soubory frontendu.
# Špatně postavená binárka se pozná až u testera prázdným oknem
# s „localhost se odmítl připojit" — a vypadá to jako rozbitá
# aplikace, ne jako rozbitý build. Klíče assetů (_app/immutable/…)
# jsou v binárce nekomprimované, takže stačí hledat je; samotné
# soubory jsou zabalené a hledat se v nich nedá.
$uiExe = Join-Path $root 'target\release\syswatch-ui.exe'
$bytes = [IO.File]::ReadAllBytes($uiExe)
$needle = [Text.Encoding]::UTF8.GetBytes('_app/immutable')
$found = $false
for ($i = 0; $i -le $bytes.Length - $needle.Length; $i++) {
    if ($bytes[$i] -eq $needle[0]) {
        $match = $true
        for ($j = 1; $j -lt $needle.Length; $j++) {
            if ($bytes[$i + $j] -ne $needle[$j]) { $match = $false; break }
        }
        if ($match) { $found = $true; break }
    }
}
if (-not $found) {
    throw "syswatch-ui.exe neobsahuje soubory frontendu — postav UI přes Tauri CLI, ne přes 'cargo build -p ui'"
}
Write-Host "     UI má vestavěný frontend" -ForegroundColor DarkGray

# ── release/ = to, co si stahuje instalátor ────────────────────────
Write-Host "3/4  Skládám release/"
$rel = Join-Path $root 'release'
New-Item -ItemType Directory -Force -Path $rel | Out-Null
Copy-Item (Join-Path $root 'target\release\syswatch.exe')    (Join-Path $rel 'syswatch.exe') -Force
Copy-Item (Join-Path $root 'target\release\syswatch-ui.exe') (Join-Path $rel 'syswatch-ui.exe') -Force
# Bez BOM a bez konce řádku — instalátor obsah jen trimuje a porovnává.
[IO.File]::WriteAllText((Join-Path $rel 'version.txt'), $version, (New-Object Text.UTF8Encoding($false)))

# Instalátor sám leží vedle: odsud si ho stáhneš ty, když ho chceš
# někomu poslat. Testeři ho už mají.
Copy-Item (Join-Path $root 'target\release\WinsentSetup.exe') (Join-Path $rel 'WinsentSetup.exe') -Force

# ── Push ───────────────────────────────────────────────────────────
Write-Host "4/4  Pushuji do repozitáře"
& git add release
& git commit -m "release: $version"
if ($LASTEXITCODE -ne 0) { Write-Host "  (nic nového k odeslání)" -ForegroundColor Yellow }
& git push origin main
if ($LASTEXITCODE -ne 0) { throw "git push selhal" }

Write-Host ""
Write-Host "Vydáno: $version" -ForegroundColor Green
Write-Host "Testeři dostanou aktualizaci spuštěním WinsentSetup.exe."
Write-Host "Instalátor k rozeslání: $(Join-Path $rel 'WinsentSetup.exe')"
