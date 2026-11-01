# Ember — Windows Port Guide

This document describes how to build, run, and distribute **Ember** on Microsoft Windows (x86_64).

---

## 1. Architecture & Port Overview

Ember is built using:
- **Rust** (2021 edition)
- **GTK 4** & **GDK Pixbuf** for the UI
- **GStreamer 1.0** (with audio plugins) for playback
- **GSettings / GIO** for configuration and resource management

### Porting Modifications Applied:
- **Portal & ashpd Isolation**: Linux portal background integration (`ashpd`) is now conditionally compiled only on Linux/FreeBSD (`#[cfg(any(target_os = "linux", target_os = "freebsd"))]`).
- **D-Bus MPRIS Decoupling & Non-Unique Flag**: The Linux D-Bus `mpris-server` crate is restricted to Unix platforms. `gio::ApplicationFlags::NON_UNIQUE` is enabled on Windows so the application does not wait for a D-Bus session bus reply.
- **GTK4 Template Correction**: Fixed GtkBuildable hierarchy in `playlist-view.ui` (`<child type="center">` instead of unsupported property), ensuring all template widgets bind properly.
- **GStreamer Plugin Curation**: Audio-focused deployment excludes GPU video/Vulkan/VA plugins (`libgstvulkan`, `libgstd3d11`, `libgstva`, etc.) that can trigger host graphics driver ICD conflicts or crashes during background plugin scanning.
- **Rendering Stability & Cairo Fallback**: `src/main.rs` and the launchers default to `GSK_RENDERER=cairo` on Windows, ensuring glitch-free 2D rendering and preventing crashes from incompatible EGL libraries loaded from system `PATH` (e.g. WezTerm's bundled `libEGL.dll`).
- **Cross-Platform Meson Runner**: Added `build-aux/cargo.py` to replace Unix-specific shell calls (`env`, `cp`, `&&`) with portable Python subprocess invocations and automatic `.exe` suffix handling.
- **Dynamic Schema & Resource Resolution**: `src/main.rs` now automatically locates `gschemas.compiled` and `amberol.gresource` in the executable directory, `./data`, or `../share/` at runtime, preventing crashes without manual environment variable configuration.
- **PulseAudio Bypass**: Unix PulseAudio property environment variables are bypassed on Windows; GStreamer automatically uses WASAPI (`wasapisink`) or DirectSound.
- **Precompiled GtkBuilder UI Definitions**: Pre-generated GTK4 builder `.ui` files are provided alongside `.blp` blueprint sources so you can compile on Windows without needing PyGObject/GIR Python bindings.

---

## 2. Setting Up the Windows Build Environment

The recommended toolchain on Windows is **MSYS2 with the UCRT64 (Universal C Runtime)** environment.

### Step 1: Install MSYS2
1. Download the MSYS2 installer from [msys2.org](https://www.msys2.org/) (or install via `winget install MSYS2.MSYS2` or `choco install msys2`).
2. Run the installer and keep the default installation folder (`C:\msys64`).

### Step 2: Install GTK4, GStreamer, and Rust in MSYS2 UCRT64
Open the **MSYS2 UCRT64** terminal from the Start menu and execute:

```bash
pacman -Syu --noconfirm
pacman -S --noconfirm \
    mingw-w64-ucrt-x86_64-toolchain \
    mingw-w64-ucrt-x86_64-gtk4 \
    mingw-w64-ucrt-x86_64-gstreamer \
    mingw-w64-ucrt-x86_64-gst-plugins-base \
    mingw-w64-ucrt-x86_64-gst-plugins-good \
    mingw-w64-ucrt-x86_64-gst-plugins-bad \
    mingw-w64-ucrt-x86_64-rust \
    mingw-w64-ucrt-x86_64-meson \
    mingw-w64-ucrt-x86_64-ninja \
    mingw-w64-ucrt-x86_64-pkgconf
```

---

## 3. Building Ember

### Method A: Automated PowerShell Build (Recommended)

From a standard PowerShell terminal in the repository root:

```powershell
# Build release binary
.\build-windows.ps1

# Or build and create a standalone distribution bundle (Ember_Winx64):
.\build-windows.ps1 -Bundle
```

The script will:
1. Detect MSYS2 UCRT64 (if installed at `C:\msys64`) and add it to `PATH`.
2. Compile GLib settings schema (`data/gschemas.compiled`).
3. Compile the resource bundle (`data/amberol.gresource`).
4. Compile the Rust binary with Cargo.
5. If `-Bundle` is passed, package `amberol.exe`, resources, schemas, and all required runtime DLLs into `dist\Ember_Winx64`.

---

### Method B: Building with Meson & Ninja

Inside the **MSYS2 UCRT64** terminal:

```bash
# Navigate to the Ember repository
cd /c/Users/<YourUsername>/Ember

# Setup build directory
meson setup build

# Compile
ninja -C build
```

---

### Method C: Direct Cargo Build

Inside the **MSYS2 UCRT64** terminal or in PowerShell with UCRT64 on PATH:

```powershell
# 1. Compile schema and resources
glib-compile-schemas data/
glib-compile-resources --target=data/amberol.gresource --sourcedir=src src/amberol.gresource.xml

# 2. Build with Cargo
cargo build --release
```

---

## 4. Running Ember on Windows

### Via the Launcher Script:
```powershell
.\run-windows.ps1
```

### Directly:
If running `target\release\amberol.exe` directly, ensure:
1. `C:\msys64\ucrt64\bin` is in your Windows `PATH` (so GTK and GStreamer DLLs are loaded).
2. Either `gschemas.compiled` is located next to `amberol.exe` or `GSETTINGS_SCHEMA_DIR` is set to `.\data`:

```powershell
$env:GSETTINGS_SCHEMA_DIR = "$PWD\data"
.\target\release\amberol.exe
```

---

## 5. Standalone Distribution Bundle (`Ember_Winx64`)

To distribute Ember to other Windows users without requiring them to install MSYS2 or Rust:

1. Run:
   ```powershell
   .\build-windows.ps1 -Bundle
   ```
2. The standalone folder `dist\Ember_Winx64\` will contain:
   ```text
   dist\Ember_Winx64\
   ├── amberol.exe
   ├── amberol.gresource
   ├── gschemas.compiled
   ├── run_ember.bat
   └── *.dll (GTK4, GStreamer, GLib, Pixbuf, and runtime DLLs)
   ```
3. Zip `dist\Ember_Winx64` or distribute it. The user can run `amberol.exe` or double-click `run_ember.bat` directly!
