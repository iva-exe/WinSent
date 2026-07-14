# dev.ps1 — jedním spuštěním celé vývojové prostředí:
#   1. self-elevace (UAC) — démon potřebuje admin práva
#   2. kontrola nástrojů (rustc, cargo, link.exe, bun, tauri CLI)
#   3. cargo build celého workspace
#   4. démon v režimu --console v samostatném okně (vidíš logy)
#   5. Tauri UI v dev režimu (hot reload) v tomto okně
#   6. Ctrl+C / zavření ukončí oba procesy korektně
#
# Použití: dvojklik na dev.bat, nebo ručně: powershell -File dev.ps1

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot

# ── 1. Self-elevace ─────────────────────────────────────────────────
$isAdmin = ([Security.Principal.WindowsPrincipal] `
    [Security.Principal.WindowsIdentity]::GetCurrent()
).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin) {
    Write-Host 'Vyžaduji elevaci (UAC)…' -ForegroundColor Yellow
    $psExe = if ($PSVersionTable.PSEdition -eq 'Core') { 'pwsh.exe' } else { 'powershell.exe' }
    Start-Process $psExe -Verb RunAs -ArgumentList @(
        '-NoExit', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`""
    )
    exit 0
}

Set-Location $root

# ── 2. Kontrola nástrojů ────────────────────────────────────────────
# cargo/bun bin nemusí být v PATH elevované session — přidáme je.
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:Path = "$cargoBin;$env:Path" }
$bunBin = Join-Path $env:USERPROFILE '.bun\bin'
if (Test-Path $bunBin) { $env:Path = "$bunBin;$env:Path" }

$missing = @()
foreach ($tool in @('rustc', 'cargo', 'bun')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { $missing += $tool }
}
# link.exe nebývá v PATH — stačí, že existuje MSVC toolset.
$linkExe = Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe" -ErrorAction SilentlyContinue
if (-not $linkExe) {
    $linkExe = Get-ChildItem "${env:ProgramFiles}\Microsoft Visual Studio\2022\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe" -ErrorAction SilentlyContinue
}
if (-not $linkExe) { $missing += 'MSVC Build Tools (link.exe)' }
if (-not (Get-Command 'cargo-tauri' -ErrorAction SilentlyContinue)) { $missing += 'tauri-cli (cargo install tauri-cli)' }

if ($missing.Count -gt 0) {
    Write-Host "Chybí nástroje: $($missing -join ', ')" -ForegroundColor Red
    Write-Host 'Doinstaluj je a spusť znovu.' -ForegroundColor Red
    Read-Host 'Enter pro konec'
    exit 1
}

# ── 3. Build workspace ──────────────────────────────────────────────
# Běžící služba drží pipe \\.\pipe\syswatch (a starší instalace i
# binárku v target\) — zastavíme ji před buildem. Po dev session ji
# případně vrátíš přes .\service.ps1 -Start.
$svc = Get-Service -Name syswatch -ErrorAction SilentlyContinue
if ($svc -and $svc.Status -ne 'Stopped') {
    Write-Host 'Zastavuji nainstalovanou službu syswatch…' -ForegroundColor Yellow
    Stop-Service -Name syswatch -Force
    $svc.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(15))
}

Write-Host '=== cargo build ===' -ForegroundColor Cyan
cargo build
if ($LASTEXITCODE -ne 0) {
    Write-Host 'Build selhal.' -ForegroundColor Red
    Read-Host 'Enter pro konec'
    exit 1
}

# ── 4. Démon v --console v samostatném okně ─────────────────────────
Write-Host '=== start démona (--console, nové okno) ===' -ForegroundColor Cyan
$daemon = Start-Process -FilePath (Join-Path $root 'target\debug\syswatch.exe') `
    -ArgumentList '--console' -PassThru

# ── 5. Tauri UI v dev režimu (blokuje toto okno) ────────────────────
# Při ukončení (Ctrl+C i zavření okna) zabijeme i démona.
try {
    Set-Location (Join-Path $root 'crates\ui')
    if (-not (Test-Path 'node_modules')) {
        Write-Host '=== bun install (poprvé) ===' -ForegroundColor Cyan
        bun install
        if ($LASTEXITCODE -ne 0) { throw 'bun install selhal' }
    }
    Write-Host '=== tauri dev (hot reload; Ctrl+C ukončí vše) ===' -ForegroundColor Cyan
    cargo tauri dev
}
finally {
    # ── 6. Úklid ────────────────────────────────────────────────────
    if ($daemon -and -not $daemon.HasExited) {
        Write-Host 'Ukončuji démona…' -ForegroundColor Yellow
        Stop-Process -Id $daemon.Id -Force -ErrorAction SilentlyContinue
    }
    Set-Location $root
}
