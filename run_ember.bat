@echo off
setlocal
cd /d "%~dp0"

if exist "%~dp0dist\Ember_Winx64\amberol.exe" goto run_dist
if exist "%~dp0target\release\amberol.exe" goto run_target

echo Error: Ember binary not found. Please run .\build-windows.ps1 first.
pause
exit /b 1

:run_dist
call "%~dp0dist\Ember_Winx64\run_ember.bat" %*
exit /b %ERRORLEVEL%

:run_target
set "GSETTINGS_SCHEMA_DIR=%~dp0data"
set "GSK_RENDERER=cairo"
set "PATH=C:\msys64\ucrt64\bin;%PATH%"
"%~dp0target\release\amberol.exe" %*
exit /b %ERRORLEVEL%
