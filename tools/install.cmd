@echo off
rem Spouštěč instalace — vyžádá si práva správce a zavolá install.ps1.
rem PowerShell skripty se v Průzkumníku samy nespouštějí, proto tenhle
rem mezikrok; -ExecutionPolicy Bypass platí jen pro tenhle jeden běh.
setlocal
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Vyzaduji prava spravce...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1"
