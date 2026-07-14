@echo off
rem Wrapper pro dvojklik — spustí dev.ps1 (ten si sám vyžádá UAC).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0dev.ps1"
