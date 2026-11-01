#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later

import os
import shutil
import subprocess
import sys

def main():
    if len(sys.argv) < 7:
        print("Usage: cargo.py <cargo_exe> <cargo_home> <target_dir> <rust_target> <project_name> <dest_output> [cargo_options...]")
        sys.exit(1)

    cargo_exe = sys.argv[1]
    cargo_home = sys.argv[2]
    target_dir = sys.argv[3]
    rust_target = sys.argv[4]
    project_name = sys.argv[5]
    dest_output = sys.argv[6]
    cargo_options = sys.argv[7:]

    env = os.environ.copy()
    env["CARGO_HOME"] = cargo_home

    cmd = [cargo_exe, "build"] + cargo_options
    print("Executing Cargo build:", " ".join(cmd))
    res = subprocess.run(cmd, env=env)
    if res.returncode != 0:
        sys.exit(res.returncode)

    exe_suffix = ".exe" if sys.platform == "win32" or os.name == "nt" else ""
    src_bin = os.path.join(target_dir, rust_target, project_name + exe_suffix)
    if not os.path.exists(src_bin):
        alt_bin = os.path.join(target_dir, rust_target, project_name)
        if os.path.exists(alt_bin):
            src_bin = alt_bin
        else:
            print(f"Error: Could not find compiled binary at {src_bin} or {alt_bin}")
            sys.exit(1)

    print(f"Copying {src_bin} -> {dest_output}")
    shutil.copyfile(src_bin, dest_output)

if __name__ == "__main__":
    main()
