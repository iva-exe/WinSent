# Odinstalace testovací verze Winsentu.
#
# Odebere službu, ETW sessions, binárky i zástupce. Nasbíraná data
# se ptá, jestli smazat — jsou tvoje a nemažou se potají.
#
# POZOR: soubor musí zůstat v UTF-8 s BOM (PowerShell 5.1, diakritika).

#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$ServiceName = 'syswatch'
$InstallDir  = Join-Path $env:ProgramFiles 'Winsent'
$DataDir     = Join-Path $env:ProgramData 'syswatch'

$principal = New-Object Security.Principal.WindowsPrincipal(
    [Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "CHYBA: spusť uninstall.cmd jako správce." -ForegroundColor Red
    Read-Host "Stiskni Enter pro zavření"
    exit 1
}

Write-Host "Winsent — odinstalace" -ForegroundColor Cyan
Write-Host ""

# ── Aplikace ───────────────────────────────────────────────────────
Get-Process syswatch-ui -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "Zavírám běžící aplikaci"
    $_ | Stop-Process -Force -ErrorAction SilentlyContinue
}

# ── Služba ─────────────────────────────────────────────────────────
if (Get-Service -Name $ServiceName -ErrorAction SilentlyContinue) {
    Write-Host "Zastavuji a odebírám službu"
    Stop-Service -Name $ServiceName -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 1500
    & sc.exe delete $ServiceName | Out-Null
} else {
    Write-Host "Služba není registrovaná (přeskakuji)"
}

# ── ETW sessions ───────────────────────────────────────────────────
# Realtime session i černá skříňka přežijí konec procesu — bez tohohle
# by po odinstalaci zůstaly viset v systému.
foreach ($s in @('syswatch-rt', 'syswatch-blackbox')) {
    & logman stop $s -ets 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) { Write-Host "Zastavena ETW session $s" }
}

# ── Soubory ────────────────────────────────────────────────────────
$lnk = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Winsent.lnk'
if (Test-Path $lnk) { Remove-Item $lnk -Force -ErrorAction SilentlyContinue }

if (Test-Path $InstallDir) {
    Write-Host "Mažu $InstallDir"
    try {
        Remove-Item $InstallDir -Recurse -Force
    } catch {
        Write-Host "Soubory drží jiný proces. Restartuj počítač a smaž $InstallDir ručně." -ForegroundColor Yellow
    }
}

# ── Data ───────────────────────────────────────────────────────────
if (Test-Path $DataDir) {
    $size = 0
    Get-ChildItem $DataDir -Recurse -File -ErrorAction SilentlyContinue |
        ForEach-Object { $size += $_.Length }
    $mb = [math]::Round($size / 1MB, 1)
    Write-Host ""
    Write-Host "Nasbíraná data ($mb MB) leží v $DataDir"
    if ((Read-Host "Smazat i je? [a/N]") -match '^[aAyY]') {
        try {
            Remove-Item $DataDir -Recurse -Force
            Write-Host "Data smazána."
        } catch {
            Write-Host "Data nejdou smazat: $_" -ForegroundColor Yellow
        }
    } else {
        Write-Host "Data zůstala na disku."
    }
}

Write-Host ""
Write-Host "Odinstalováno." -ForegroundColor Green
Write-Host ""
Read-Host "Stiskni Enter pro zavření"
