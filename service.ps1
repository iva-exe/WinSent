# service.ps1 — správa Windows služby syswatch.
#
# Použití (z elevovaného PowerShellu; skript si elevaci vyžádá sám):
#   .\service.ps1 -Install     registrace služby + Service Recovery watchdog
#   .\service.ps1 -Uninstall   zastavení a odregistrace
#   .\service.ps1 -Start       spuštění
#   .\service.ps1 -Stop        zastavení
#   .\service.ps1 -Status      stav služby + recovery konfigurace

param(
    [switch]$Install,
    [switch]$Uninstall,
    [switch]$Start,
    [switch]$Stop,
    [switch]$Status
)

$ErrorActionPreference = 'Stop'
$serviceName = 'syswatch'
$exePath = Join-Path $PSScriptRoot 'target\debug\syswatch.exe'

# Status jen čte — nepotřebuje elevaci. Vše ostatní ano.
$needsAdmin = $Install -or $Uninstall -or $Start -or $Stop
$isAdmin = ([Security.Principal.WindowsPrincipal] `
    [Security.Principal.WindowsIdentity]::GetCurrent()
).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if ($needsAdmin -and -not $isAdmin) {
    Write-Host 'Vyžaduji elevaci (UAC)…' -ForegroundColor Yellow
    $psExe = if ($PSVersionTable.PSEdition -eq 'Core') { 'pwsh.exe' } else { 'powershell.exe' }
    $args = @('-NoExit', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"")
    foreach ($p in $PSBoundParameters.Keys) { $args += "-$p" }
    Start-Process $psExe -Verb RunAs -ArgumentList $args
    exit 0
}

function Show-Status {
    $svc = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    if (-not $svc) {
        Write-Host "Služba '$serviceName' není nainstalovaná." -ForegroundColor Yellow
        return
    }
    Write-Host "Služba:  $($svc.DisplayName)"
    Write-Host "Stav:    $($svc.Status)"
    Write-Host "Start:   $((Get-CimInstance Win32_Service -Filter "Name='$serviceName'").StartMode)"
    Write-Host '--- Service Recovery (sc qfailure) ---'
    sc.exe qfailure $serviceName
}

if ($Install) {
    if (-not (Test-Path $exePath)) {
        Write-Host "Binárka $exePath neexistuje — nejdřív spusť cargo build (nebo dev.bat)." -ForegroundColor Red
        exit 1
    }
    if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) {
        Write-Host 'Služba už existuje — nejdřív -Uninstall.' -ForegroundColor Yellow
        exit 1
    }

    # Registrace: auto start, LocalSystem (default), argument --service.
    Write-Host "Instaluji službu '$serviceName'…" -ForegroundColor Cyan
    sc.exe create $serviceName binPath= "`"$exePath`" --service" start= auto DisplayName= "syswatch — systémový monitor" | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Host 'sc create selhalo.' -ForegroundColor Red; exit 1 }
    sc.exe description $serviceName "Démon systémového monitoru syswatch (v0 skelet)." | Out-Null

    # Service Recovery watchdog (SPEC kap. 2.3): OS restartuje službu po
    # pádu — 5 s, 10 s, 30 s; čítač selhání se nuluje po 24 h.
    sc.exe failure $serviceName reset= 86400 actions= restart/5000/restart/10000/restart/30000 | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Host 'sc failure selhalo.' -ForegroundColor Red; exit 1 }
    # I pády, které služba „ohlásí“ nenulovým exit kódem, počítej jako selhání.
    sc.exe failureflag $serviceName 1 | Out-Null

    Write-Host 'Nainstalováno vč. Service Recovery. Spouštím…' -ForegroundColor Green
    sc.exe start $serviceName | Out-Null
    Show-Status
}
elseif ($Uninstall) {
    $svc = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    if (-not $svc) { Write-Host 'Služba není nainstalovaná.'; exit 0 }
    if ($svc.Status -ne 'Stopped') {
        Write-Host 'Zastavuji službu…' -ForegroundColor Cyan
        Stop-Service -Name $serviceName -Force
        $svc.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }
    sc.exe delete $serviceName | Out-Null
    Write-Host 'Služba odregistrována.' -ForegroundColor Green
}
elseif ($Start) {
    Start-Service -Name $serviceName
    Show-Status
}
elseif ($Stop) {
    Stop-Service -Name $serviceName -Force
    Show-Status
}
elseif ($Status) {
    Show-Status
}
else {
    Write-Host 'Použití: service.ps1 -Install | -Uninstall | -Start | -Stop | -Status'
}
