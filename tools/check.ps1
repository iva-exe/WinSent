# Spustí brány (kontroly definice hotového) najednou a rychle.
#
#   powershell -ExecutionPolicy Bypass -File tools\check.ps1
#   powershell -ExecutionPolicy Bypass -File tools\check.ps1 memcheck v9usercheck
#
# Proč to nespouštět přes `cargo run --release --example X`:
# release profil má `lto = "thin"`, takže KAŽDÁ brána prolinkuje celý
# strom závislostí zvlášť — osm bran znamenalo osm linkování za sebou.
# Brány jsou přitom krátkodobí klienti pipe, kterým je optimalizace
# k ničemu. Tenhle skript je postaví jednou v ladicím profilu (bez LTO)
# a pak spustí hotové binárky paralelně.
#
# POZOR na kódování: soubor musí zůstat v UTF-8 s BOM (PowerShell 5.1).

#Requires -Version 5.1
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Only)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# Brány v pořadí, v jakém přibývaly. Bez argumentu se pustí všechny.
$all = @(
    'ping', 'snapshot', 'history', 'v5check', 'rescancheck', 'startupcheck',
    'auditcheck', 'killcheck', 'filecheck', 'cleanupcheck', 'appcheck',
    'iconcheck', 'incidents', 'v8check', 'v8dcheck', 'v9check', 'v9netcheck',
    'v9conncheck', 'v9seccheck', 'v9usercheck', 'permusecheck', 'v10check', 'memcheck',
    'onstartcheck', 'updatecheck', 'purgecheck', 'gpucheck', 'permlive',
    'hwgroupcheck', 'idcheck', 'netcheck'
)
$gates = if ($Only) { $Only } else { $all }

Write-Host "Winsent — brány ($($gates.Count))" -ForegroundColor Cyan

# ── Build (jednou, pro všechny) ────────────────────────────────────
$t0 = [Diagnostics.Stopwatch]::StartNew()
& cargo build -q -p ipc -p identity -p win-sys --examples
if ($LASTEXITCODE -ne 0) { throw "build bran selhal ($LASTEXITCODE)" }
Write-Host ("  build {0:N0} s" -f $t0.Elapsed.TotalSeconds) -ForegroundColor DarkGray

# ── Běh (paralelně) ────────────────────────────────────────────────
# Brány jen čtou přes pipe, takže si navzájem nepřekážejí. Služba je
# obsluhuje po jedné, ale každá je otázka milisekund.
$dir = Join-Path $root 'target\debug\examples'
$jobs = @()
foreach ($g in $gates) {
    $exe = Join-Path $dir ($g + ".exe")
    if (-not (Test-Path $exe)) {
        Write-Host "  ?  $g — binárka není (překlep v názvu?)" -ForegroundColor Yellow
        continue
    }
    $jobs += Start-Job -ArgumentList $exe, $g -ScriptBlock {
        param($exe, $name)
        # Brány píšou česky v UTF-8; bez tohohle je job dekóduje
        # kódovou stránkou konzole a diakritika se rozsype.
        [Console]::OutputEncoding = [Text.Encoding]::UTF8
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $out = & $exe 2>&1
        [pscustomobject]@{
            Name    = $name
            Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)
            Code    = $LASTEXITCODE
            Last    = ($out | Select-Object -Last 1)
            Output  = ($out -join "`n")
        }
    }
}

$results = @($jobs | Wait-Job | Receive-Job)
$jobs | Remove-Job

# ── Výsledek ───────────────────────────────────────────────────────
$failed = @()
foreach ($r in ($results | Sort-Object Name)) {
    $ok = $r.Code -eq 0
    if (-not $ok) { $failed += $r }
    $color = if ($ok) { 'Green' } else { 'Red' }
    Write-Host ("  {0,-14} {1,5:N1} s  {2}" -f $r.Name, $r.Seconds, $r.Last) -ForegroundColor $color
}

Write-Host ""
Write-Host ("Celkem {0:N0} s · {1} z {2} prošlo" -f $t0.Elapsed.TotalSeconds, ($results.Count - $failed.Count), $results.Count) -ForegroundColor Cyan

# Výpis padlých bran celý — jinak by se muselo pouštět znovu ručně.
foreach ($f in $failed) {
    Write-Host ""
    Write-Host "─── $($f.Name) ───" -ForegroundColor Red
    Write-Host $f.Output
}
if ($failed.Count -gt 0) { exit 1 }
