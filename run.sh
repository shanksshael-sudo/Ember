#!/bin/sh
cd "$(dirname "$0")"

# Setup build directory if missing
if [ ! -d "build" ]; then
    echo "Setting up build directory..."
    meson setup build
fi

# Recompile the app
echo "Compiling Ember..."
ninja -C build

# Run the compiled binary with correct settings
export MESON_DEVENV=1
export GSETTINGS_SCHEMA_DIR="$(find build/ -iname gschemas.compiled -exec dirname {} \; | head -1)"
./build/src/amberol
