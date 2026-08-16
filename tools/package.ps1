# Sestaví balíček testovací verze pro poslání někomu dalšímu.
#
# Projekt má na disku ~14 GB, ale to je z drtivé většiny `target/` —
# kompilovací cache Rustu, která se NIKDY nedistribuuje. Ven jdou dvě
# binárky a instalační skripty, dohromady jednotky MB.
#
# Použití:  powershell -ExecutionPolicy Bypass -File tools\package.ps1
# Výsledek: dist\Winsent-test-<verze>-<datum>.zip

#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# Verze z workspace Cargo.toml. Hledá se v sekci [workspace.package],
# protože 'version = ' se v souboru vyskytuje i u závislostí.
$cargo = Get-Content (Join-Path $root 'Cargo.toml')
$inPkg = $false
$version = $null
foreach ($line in $cargo) {
    if ($line -match '^\s*\[workspace\.package\]') { $inPkg = $true; continue }
    if ($inPkg -and $line -match '^\s*\[') { break }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $version = $Matches[1]; break }
}
if (-not $version) { throw "verzi se nepodařilo přečíst z Cargo.toml" }

$stamp = Get-Date -Format 'yyyyMMdd'
$name  = "Winsent-test-$version-$stamp"
$dist  = Join-Path $root 'dist'
$stage = Join-Path $dist $name

Write-Host "Winsent — balení testovací verze $version" -ForegroundColor Cyan

# ── Build ──────────────────────────────────────────────────────────
Write-Host "1/4  Sestavuji frontend"
Push-Location (Join-Path $root 'crates\ui')
& npm.cmd run build
$npmCode = $LASTEXITCODE
Pop-Location
if ($npmCode -ne 0) { throw "npm run build selhal (kód $npmCode)" }

Write-Host "2/4  Sestavuji binárky (release)"
& cargo build --release -p svc -p ui
if ($LASTEXITCODE -ne 0) { throw "cargo build selhal (kód $LASTEXITCODE)" }

# ── Staging ────────────────────────────────────────────────────────
Write-Host "3/4  Skládám balíček"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

$files = @(
    'target\release\syswatch.exe',
    'target\release\syswatch-ui.exe',
    'tools\install.cmd',
    'tools\install.ps1',
    'tools\uninstall.cmd',
    'tools\uninstall.ps1',
    'tools\PRECTI-ME.txt'
)
foreach ($rel in $files) {
    $s = Join-Path $root $rel
    if (-not (Test-Path $s)) { throw "chybí $rel" }
    Copy-Item $s (Join-Path $stage (Split-Path -Leaf $rel)) -Force
}

# ── ZIP ────────────────────────────────────────────────────────────
# Balí se PODSLOŽKA (ne její obsah), aby po rozbalení vznikla jedna
# složka a soubory se nerozsypaly do Downloads.
Write-Host "4/4  Balím do ZIPu"
$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal

$mb = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host ""
Write-Host "Hotovo: $zip  ($mb MB)" -ForegroundColor Green
Write-Host ""
Write-Host "Obsah:"
Get-ChildItem $stage | ForEach-Object {
    "  {0,-22} {1,8:N0} kB" -f $_.Name, ($_.Length / 1KB)
}
