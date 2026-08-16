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
Write-Host "1/4  Frontend"
Push-Location (Join-Path $root 'crates\ui')
& npm.cmd run build
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) { throw "npm run build selhal ($code)" }

Write-Host "2/4  Binárky (release)"
& cargo build --release -p svc -p ui -p installer
if ($LASTEXITCODE -ne 0) { throw "cargo build selhal ($LASTEXITCODE)" }

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
