# Ember Windows Launcher
# SPDX-License-Identifier: GPL-3.0-or-later

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
Set-Location $ScriptDir

# MSYS2 UCRT64 PATH addition for DLL resolution
$MsysUcrtBin = "C:\msys64\ucrt64\bin"
if (Test-Path $MsysUcrtBin) {
    if ($env:PATH -notlike "*$MsysUcrtBin*") {
        $env:PATH = "$MsysUcrtBin;" + $env:PATH
    }
}

# Locate compiled binary
$Candidates = @(
    "dist\Ember_Winx64\amberol.exe",
    "target\release\amberol.exe",
    "target\debug\amberol.exe",
    "build\src\amberol.exe"
)

$ExePath = $null
foreach ($path in $Candidates) {
    if (Test-Path $path) {
        $ExePath = (Resolve-Path $path).Path
        break
    }
}

if (-not $ExePath) {
    Write-Error "No compiled Ember executable found. Please run .\build-windows.ps1 first."
}

# Configure schema directory
if (Test-Path "data\gschemas.compiled") {
    $env:GSETTINGS_SCHEMA_DIR = (Resolve-Path "data").Path
} elseif (Test-Path "$((Split-Path $ExePath))\gschemas.compiled") {
    $env:GSETTINGS_SCHEMA_DIR = Split-Path $ExePath
}

if (-not $env:GSK_RENDERER) {
    $env:GSK_RENDERER = "cairo"
}

Write-Host "[*] Launching Ember ($ExePath)..." -ForegroundColor Cyan
& $ExePath $args
