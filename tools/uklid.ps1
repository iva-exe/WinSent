# Úklid vývojové složky — drží target/ malý.
#
#   powershell -ExecutionPolicy Bypass -File tools\uklid.ps1 [-LimitGB 12]
#
# Cargo staré verze přeložených knihoven nikdy nemaže: každá změna
# přidá nové soubory s jiným otiskem a staré zůstanou. Složka projektu
# tak na vývojovém stroji dorostla na 39 GB (38 GB target/), na disku,
# kde je místa málo. Skript běží na konci tools\publish.ps1:
#
#   1. vždy smaže inkrementální mezipaměti (debug i release) — urychlují
#      jen opakované drobné změny v rámci jedné práce a po vydání se stejně
#      přeloží znovu; bývají z nich gigabajty,
#   2. když je target/ i potom větší než limit, smaže ho celý (cargo
#      clean). Příští build pak trvá pár minut déle, ale složka se vrátí
#      na čerstvou velikost (~3 GB testy + ~3 GB release).
#
# POZOR na kódování: soubor musí zůstat v UTF-8 s BOM (PowerShell 5.1).

#Requires -Version 5.1
param([double]$LimitGB = 12)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$target = Join-Path $root 'target'
if (-not (Test-Path $target)) {
    Write-Host "Úklid: target/ neexistuje, není co uklízet." -ForegroundColor DarkGray
    return
}

function Velikost([string]$cesta) {
    if (-not (Test-Path $cesta)) { return 0 }
    $s = (Get-ChildItem -LiteralPath $cesta -Recurse -File -Force -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum
    if ($s) { [double]$s } else { 0 }
}
$GB = 1GB
$pred = Velikost $target

foreach ($profil in 'debug', 'release') {
    $inc = Join-Path $target "$profil\incremental"
    if (Test-Path $inc) {
        Remove-Item -LiteralPath $inc -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$po = Velikost $target
if ($po / $GB -gt $LimitGB) {
    Write-Host ("Úklid: target/ má {0:N1} GB (limit {1} GB) — mažu celý (cargo clean)." -f ($po / $GB), $LimitGB)
    Push-Location $root
    try { & cargo clean 2>&1 | Out-Null } finally { Pop-Location }
    $po = Velikost $target
}

Write-Host ("Úklid: target/ {0:N1} GB → {1:N1} GB" -f ($pred / $GB), ($po / $GB)) -ForegroundColor DarkGray

# Git: každé vydání přidá do historie nové binárky a objekty zůstávaly
# nezabalené (4 200 kusů, 390 MB; po zabalení 88 MB). Automatické
# `git gc` se tu nikdy nespustilo, proto se balí ručně, když se jich
# nasbírá víc.
Push-Location $root
try {
    $volne = (& git count-objects -v | Select-String '^count: (\d+)').Matches.Groups[1].Value
    if ([int]$volne -gt 1000) {
        & git gc --quiet
        Write-Host "Úklid: git zabalen ($volne volných objektů)." -ForegroundColor DarkGray
    }
} catch {
    Write-Host "Úklid: git gc se nepovedl: $_" -ForegroundColor Yellow
} finally { Pop-Location }
