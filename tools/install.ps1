# Instalace testovací verze Winsentu.
#
# Co to udělá:
#   1. zkopíruje dvě binárky do C:\Program Files\Winsent
#   2. zaregistruje službu syswatch (LocalSystem, automatický start)
#   3. nastaví automatický restart služby při pádu
#   4. spustí službu a udělá zástupce v nabídce Start
#
# Nic se neposílá po síti, nic se nestahuje. Odinstalace je jeden
# příkaz — viz uninstall.cmd ve stejné složce.
#
# POZOR na kódování: soubor MUSÍ zůstat v UTF-8 s BOM. PowerShell 5.1
# (výchozí na Windows 10) čte skripty bez BOM jako ANSI a diakritika
# ve výpisech se rozsype na nečitelné znaky.

#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$ServiceName = 'syswatch'
$DisplayName = 'Winsent — systémový monitor'
$Description = 'Sbírá metriky systému pro aplikaci Winsent. Data zůstávají v tomto počítači.'
$InstallDir  = Join-Path $env:ProgramFiles 'Winsent'
$DataDir     = Join-Path $env:ProgramData 'syswatch'

function Fail($msg) {
    Write-Host ""
    Write-Host "CHYBA: $msg" -ForegroundColor Red
    Write-Host ""
    Read-Host "Stiskni Enter pro zavření"
    exit 1
}

# ── Práva správce ──────────────────────────────────────────────────
$principal = New-Object Security.Principal.WindowsPrincipal(
    [Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Fail "Skript musí běžet jako správce. Klikni na install.cmd pravým tlačítkem → Spustit jako správce."
}

$src = Split-Path -Parent $MyInvocation.MyCommand.Path
$svcSrc = Join-Path $src 'syswatch.exe'
$uiSrc  = Join-Path $src 'syswatch-ui.exe'
foreach ($f in @($svcSrc, $uiSrc)) {
    if (-not (Test-Path $f)) {
        Fail "Chybí $(Split-Path -Leaf $f) — rozbal celý archiv, ne jen skript."
    }
}

Write-Host "Winsent — instalace testovací verze" -ForegroundColor Cyan
Write-Host ""

# ── WebView2: rozhraní aplikace na něm stojí ───────────────────────
# Evergreen runtime má Win11 i aktualizovaná Win10; na starších
# strojích chybí a okno aplikace by zůstalo prázdné.
$wv2Client = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$wv2Keys = @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$wv2Client",
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$wv2Client",
    "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$wv2Client"
)
$hasWv2 = $false
foreach ($k in $wv2Keys) {
    $pv = (Get-ItemProperty -Path $k -Name pv -ErrorAction SilentlyContinue).pv
    if ($pv -and $pv -ne '0.0.0.0') { $hasWv2 = $true; break }
}
if (-not $hasWv2) {
    Write-Host "POZOR: chybí WebView2 Runtime — okno aplikace by zůstalo prázdné." -ForegroundColor Yellow
    Write-Host "       Zdarma přímo od Microsoftu:"
    Write-Host "       https://go.microsoft.com/fwlink/p/?LinkId=2124703"
    Write-Host ""
    if ((Read-Host "Pokračovat i tak? [a/N]") -notmatch '^[aAyY]') { exit 1 }
}

# ── Odstranění staré instalace ─────────────────────────────────────
# Službu vždy smažeme a založíme znovu: `Set-Service` v PowerShellu 5.1
# neumí změnit cestu k binárce a `sc.exe config` se s uvozovkami
# v cestě „C:\Program Files\…" pere natolik, že to nestojí za riziko.
$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "Odstraňuji předchozí instalaci služby…"
    Stop-Service -Name $ServiceName -Force -ErrorAction SilentlyContinue
    & sc.exe delete $ServiceName | Out-Null
    # SCM drží službu „označenou ke smazání", dokud na ni má někdo
    # otevřený handle (typicky services.msc). Chvíli počkáme.
    for ($i = 0; $i -lt 20; $i++) {
        Start-Sleep -Milliseconds 500
        if (-not (Get-Service -Name $ServiceName -ErrorAction SilentlyContinue)) { break }
    }
    if (Get-Service -Name $ServiceName -ErrorAction SilentlyContinue) {
        Fail "Starou službu nejde odebrat. Zavři Služby (services.msc) a spusť instalaci znovu."
    }
}

# ── Kopie binárek ──────────────────────────────────────────────────
Write-Host "Kopíruji soubory do $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$svcExe = Join-Path $InstallDir 'syswatch.exe'
$uiExe  = Join-Path $InstallDir 'syswatch-ui.exe'
try {
    Copy-Item $svcSrc $svcExe -Force
    Copy-Item $uiSrc  $uiExe  -Force
} catch {
    Fail "Soubory nejdou zkopírovat: $_`nZavři běžící Winsent a zkus to znovu."
}

# ── Registrace služby ──────────────────────────────────────────────
# New-Service si s mezerami v cestě poradí sám (na rozdíl od sc.exe).
Write-Host "Registruji službu $ServiceName"
New-Service -Name $ServiceName `
    -BinaryPathName ('"{0}" --service' -f $svcExe) `
    -DisplayName $DisplayName `
    -Description $Description `
    -StartupType Automatic | Out-Null

# Pád služby nesmí znamenat konec sběru — tři pokusy o restart.
# (Argumenty bez mezer, takže sc.exe je tady bezpečné.)
& sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/10000/restart/30000 | Out-Null

Write-Host "Spouštím službu"
try {
    Start-Service -Name $ServiceName
} catch {
    Fail "Služba se nespustila: $_`nLog najdeš v $DataDir\logs\svc.log"
}
Start-Sleep -Seconds 2
$state = (Get-Service -Name $ServiceName).Status
if ($state -ne 'Running') {
    Fail "Služba se nerozeběhla (stav: $state). Log: $DataDir\logs\svc.log"
}

# ── Zástupce v nabídce Start ───────────────────────────────────────
try {
    $lnk = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Winsent.lnk'
    $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
    $shortcut.TargetPath = $uiExe
    $shortcut.WorkingDirectory = $InstallDir
    $shortcut.Description = 'Winsent — správa a monitoring Windows'
    $shortcut.Save()
} catch {
    Write-Host "Zástupce se nepodařilo vytvořit (aplikace jde spustit ze složky)." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Hotovo." -ForegroundColor Green
Write-Host "  Služba:   $ServiceName ($state)"
Write-Host "  Aplikace: nabídka Start → Winsent"
Write-Host "  Data:     $DataDir  (zůstávají v tomto počítači)"
Write-Host ""
Write-Host "Odinstalace: pravým na uninstall.cmd → Spustit jako správce"
Write-Host ""

if ((Read-Host "Spustit aplikaci teď? [A/n]") -notmatch '^[nN]') {
    # Aplikace MUSÍ běžet pod běžným uživatelem, ne jako správce
    # (SPEC kap. 2.1) — z elevovaného okna by se jinak zdědila práva.
    # Explorer běží pod uživatelem, takže spuštění „přes něj" vrátí
    # práva na normální úroveň.
    try {
        Start-Process explorer.exe -ArgumentList "`"$uiExe`""
    } catch {
        Write-Host "Spusť ji prosím z nabídky Start." -ForegroundColor Yellow
    }
}
