@echo off
rem Spouštěč odinstalace — vyžádá si práva správce a zavolá uninstall.ps1.
setlocal
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Vyzaduji prava spravce...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1"
