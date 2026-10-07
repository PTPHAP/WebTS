@echo off
python "%~dp0linker.py" %*
exit /b %errorlevel%
