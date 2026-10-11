@echo off
setlocal
set PSModulePath=
chcp 65001 >nul
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\windows-menu.ps1" %*
