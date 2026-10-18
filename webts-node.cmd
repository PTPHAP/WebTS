@echo off
setlocal
if "%~1"=="" goto install
if /i "%~1"=="install" goto install
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\node.ps1" %*
exit /b %errorlevel%
:install
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\node-install.ps1"
exit /b %errorlevel%
