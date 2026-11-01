# SPDX-FileCopyrightText: 2022  Emmanuele Bassi
# SPDX-License-Identifier: GPL-3.0-or-later

subdir('gtk')

pkgdatadir = get_option('prefix') / get_option('datadir') / meson.project_name()

gnome.compile_resources('amberol',
  'amberol.gresource.xml',
  gresource_bundle: true,
  dependencies: blueprints,
  install: true,
  install_dir: pkgdatadir,
)

conf = configuration_data()
conf.set_quoted('VERSION', '@0@@1@'.format(meson.project_version(), version_suffix))
conf.set_quoted('GETTEXT_PACKAGE', 'amberol')
conf.set_quoted('LOCALEDIR', get_option('prefix') / get_option('localedir'))
conf.set_quoted('PKGDATADIR', pkgdatadir)
conf.set_quoted('APPLICATION_ID', application_id)
conf.set_quoted('PROFILE', get_option('profile'))

config_rs = configure_file(
  input: 'config.rs.in',
  output: 'config.rs',
  configuration: conf
)

python3 = import('python').find_installation('python3')

# Copy the config.rs output to the source directory.
run_command(
  python3, '-c',
  'import shutil, sys; shutil.copyfile(sys.argv[1], sys.argv[2])',
  meson.project_build_root() / 'src' / 'config.rs',
  meson.project_source_root() / 'src' / 'config.rs',
  check: true,
)

cargo_options = [ '--manifest-path', meson.project_source_root() / 'Cargo.toml' ]
cargo_options += [ '--target-dir', meson.project_build_root() / 'src' ]

if get_option('profile') == 'default'
  cargo_options += [ '--release' ]
  rust_target = 'release'
  message('Building in release mode')
else
  rust_target = 'debug'
  message('Building in debug mode')
endif

cargo_wrapper = meson.project_source_root() / 'build-aux' / 'cargo.py'

cargo_release = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: host_machine.system() == 'windows' ? '@0@.exe'.format(meson.project_name()) : meson.project_name(),
  console: true,
  install: true,
  install_dir: get_option('bindir'),
  command: [
    python3,
    cargo_wrapper,
    cargo,
    meson.project_build_root() / 'cargo-home',
    meson.project_build_root() / 'src',
    rust_target,
    meson.project_name(),
    '@OUTPUT@',
    cargo_options,
  ],
)
