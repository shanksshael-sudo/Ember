# Ember Windows Build Script
# SPDX-License-Identifier: GPL-3.0-or-later

param (
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [switch]$Bundle
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
Set-Location $ScriptDir

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "       Ember Windows Build System       " -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# 1. Environment Detection (Check for MSYS2 UCRT64 or Native Tools)
$MsysUcrtBin = "C:\msys64\ucrt64\bin"
$MsysUsrBin = "C:\msys64\usr\bin"
if (Test-Path $MsysUcrtBin) {
    Write-Host "[*] Detected MSYS2 UCRT64 at $MsysUcrtBin. Adding to PATH..." -ForegroundColor Green
    if ($env:PATH -notlike "*$MsysUcrtBin*") {
        $env:PATH = "$MsysUcrtBin;$MsysUsrBin;" + $env:PATH
    }
}

# 2. Check for required build tools
$HasCargo = (Get-Command cargo -ErrorAction SilentlyContinue) -ne $null
$HasSchemaCompiler = (Get-Command glib-compile-schemas -ErrorAction SilentlyContinue) -ne $null
$HasResourceCompiler = (Get-Command glib-compile-resources -ErrorAction SilentlyContinue) -ne $null

Write-Host "[*] Checking tools:"
Write-Host "    - Cargo: $(if ($HasCargo) { 'OK' } else { 'Missing (install Rust/cargo)' })"
Write-Host "    - glib-compile-schemas: $(if ($HasSchemaCompiler) { 'OK' } else { 'Missing (install GTK4 / MSYS2)' })"
Write-Host "    - glib-compile-resources: $(if ($HasResourceCompiler) { 'OK' } else { 'Missing (install GTK4 / MSYS2)' })"

if (-not $HasCargo) {
    Write-Error "Cargo was not found in PATH. Please install Rust from https://rustup.rs or via MSYS2."
}

# 3. Ensure fallback config.rs exists
if (-not (Test-Path "src\config.rs")) {
    Write-Host "[*] Generating fallback src\config.rs..." -ForegroundColor Yellow
    $configContent = @(
        '// Auto-generated configuration for Ember',
        'pub static VERSION: &str = "2026.7.0";',
        'pub static GETTEXT_PACKAGE: &str = "amberol";',
        'pub static LOCALEDIR: &str = "/usr/share/locale";',
        'pub static PKGDATADIR: &str = "/usr/share/amberol";',
        'pub static APPLICATION_ID: &str = "io.bassi.Amberol";',
        'pub static PROFILE: &str = "";'
    ) -join "`n"
    Set-Content -Path "src\config.rs" -Value $configContent -Encoding utf8
}

# 4. Compile GLib schemas (merging application and GTK schemas so file/color dialogs function)
if ($HasSchemaCompiler) {
    Write-Host "[*] Compiling GLib schemas..." -ForegroundColor Green
    $SchemaStaging = "build\schemas"
    New-Item -ItemType Directory -Force -Path $SchemaStaging | Out-Null
    $SystemSchemas = "C:\msys64\ucrt64\share\glib-2.0\schemas"
    if (Test-Path $SystemSchemas) {
        Copy-Item "$SystemSchemas\*.xml" -Destination $SchemaStaging -Force
    }
    Copy-Item "data\io.bassi.Amberol.gschema.xml" -Destination $SchemaStaging -Force
    glib-compile-schemas $SchemaStaging
    Copy-Item "$SchemaStaging\gschemas.compiled" -Destination "data\gschemas.compiled" -Force
} else {
    Write-Host "[!] Skipping schema compilation (glib-compile-schemas not available)." -ForegroundColor Yellow
}

# 5. Compile GResource bundle
if ($HasResourceCompiler) {
    Write-Host "[*] Compiling GResource bundle (amberol.gresource)..." -ForegroundColor Green
    if (-not (Test-Path "data")) { New-Item -ItemType Directory -Path "data" | Out-Null }
    glib-compile-resources --target="data/amberol.gresource" --sourcedir="src" "src/amberol.gresource.xml"
} else {
    Write-Host "[!] Skipping resource compilation (glib-compile-resources not available)." -ForegroundColor Yellow
}

# 6. Build with Cargo
Write-Host "[*] Compiling Ember binary with Cargo ($Configuration mode)..." -ForegroundColor Green
$CargoArgs = @("build")
if ($Configuration -eq "release") {
    $CargoArgs += "--release"
}

& cargo @CargoArgs
if ($LASTEXITCODE -ne 0) {
    Write-Error "Cargo build failed with exit code $LASTEXITCODE"
}

# 7. Locate output binary
$TargetDir = if ($Configuration -eq "release") { "target\release" } else { "target\debug" }
$BinPath = "$TargetDir\amberol.exe"
if (-not (Test-Path $BinPath)) {
    if (Test-Path "$TargetDir\amberol") {
        $BinPath = "$TargetDir\amberol"
    }
}

Write-Host "`n[OK] Build successful! Binary located at: $BinPath" -ForegroundColor Green

# 8. Optional: Bundle for distribution
if ($Bundle -or $PSBoundParameters['Bundle']) {
    $DistDir = "dist\Ember_Winx64"
    Write-Host "[*] Creating distribution bundle at $DistDir..." -ForegroundColor Cyan
    if (Test-Path $DistDir) { Remove-Item $DistDir -Recurse -Force }
    New-Item -ItemType Directory -Path $DistDir | Out-Null

    Copy-Item $BinPath -Destination "$DistDir\amberol.exe"

    if (Test-Path "data\amberol.gresource") {
        Copy-Item "data\amberol.gresource" -Destination "$DistDir\amberol.gresource"
    }
    if (Test-Path "data\gschemas.compiled") {
        Copy-Item "data\gschemas.compiled" -Destination "$DistDir\gschemas.compiled"
    }
    if (Test-Path "src\assets\fonts") {
        New-Item -ItemType Directory -Force -Path "$DistDir\fonts" | Out-Null
        Copy-Item "src\assets\fonts\*.ttf" -Destination "$DistDir\fonts\" -Force
    }

    # Copy MSYS2 DLLs if we are in MSYS2 environment
    if (Test-Path $MsysUcrtBin) {
        Write-Host "[*] Resolving and bundling all runtime DLLs from $MsysUcrtBin..." -ForegroundColor Green
        $NtlddExe = "$MsysUcrtBin\ntldd.exe"
        if (Test-Path $NtlddExe) {
            $dllLines = & $NtlddExe -R "$DistDir\amberol.exe"
            $dllPaths = $dllLines | ForEach-Object {
                if ($_ -match "=>\s+(.*\\ucrt64\\bin\\[^ ]+\.dll)") {
                    $matches[1]
                }
            } | Select-Object -Unique
            foreach ($dll in $dllPaths) {
                if (Test-Path $dll) {
                    Copy-Item $dll -Destination $DistDir -Force
                }
            }
        } else {
            $GtkDlls = Get-ChildItem $MsysUcrtBin -Filter "*.dll" | Where-Object {
                $_.Name -match "^(libgtk-4|libadwaita|libgstreamer|libgst|libglib|libgio|libgobject|libgdk|libpango|libcairo|libffi|libintl|zlib|libpng|libjpeg|libepoxy|libgraphene|libwinpthread|libstdc|libgcc|libfontconfig|libfreetype|libharfbuzz|libfribidi|libpixman|libexpat|libbz2|libbrotli|libzstd|libdeflate|libtiff|libwebp|liblzma|libpcre2)"
            }
            foreach ($dll in $GtkDlls) {
                Copy-Item $dll.FullName -Destination $DistDir -Force
            }
        }

        # Copy GStreamer plugins (excluding GPU/video hardware plugins that can conflict with graphics drivers)
        $GstPluginDir = "C:\msys64\ucrt64\lib\gstreamer-1.0"
        if (Test-Path $GstPluginDir) {
            Write-Host "[*] Bundling GStreamer plugins..." -ForegroundColor Green
            $DestGst = "$DistDir\lib\gstreamer-1.0"
            New-Item -ItemType Directory -Force -Path $DestGst | Out-Null
            Get-ChildItem "$GstPluginDir\*.dll" | Where-Object {
                ($_.Name -notmatch "vulkan|d3d|cuda|nvcodec|va|amf") -or ($_.Name -match "id3")
            } | ForEach-Object {
                Copy-Item $_.FullName -Destination $DestGst -Force
            }
        }
    }

    # Create run shortcut / launcher batch file
    $batLines = @(
        '@echo off',
        'setlocal',
        'cd /d "%~dp0"',
        'set "GSETTINGS_SCHEMA_DIR=%~dp0"',
        'set "GSK_RENDERER=cairo"',
        'set "PATH=%~dp0;%PATH%"',
        '"%~dp0amberol.exe" %*'
    ) -join "`r`n"
    Set-Content -Path "$DistDir\run_ember.bat" -Value $batLines -Encoding ascii

    Write-Host "[OK] Distribution bundle created at $DistDir" -ForegroundColor Green
}

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "To run Ember on Windows, execute:" -ForegroundColor White
Write-Host "    powershell -ExecutionPolicy Bypass -File .\run-windows.ps1" -ForegroundColor Yellow
Write-Host "========================================" -ForegroundColor Cyan
