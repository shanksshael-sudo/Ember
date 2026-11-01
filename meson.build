# SPDX-FileCopyrightText: 2022  Emmanuele Bassi
# SPDX-License-Identifier: GPL-3.0-or-later

project('amberol', 'rust',
  version: '2026.2',
  license: ['GPL-3.0'],
  meson_version: '>= 0.59.0',
  default_options: [ 'warning_level=2', ],
)

dependency('gtk4', version: '>= 4.19.5')
dependency('gstreamer-1.0', version: '>= 1.20')
dependency('gstreamer-audio-1.0', version: '>= 1.20')
dependency('gstreamer-play-1.0', version: '>= 1.20')
dependency('gstreamer-plugins-base-1.0', version: '>= 1.20')
dependency('gstreamer-plugins-bad-1.0', version: '>= 1.20')
dependency('gstreamer-bad-audio-1.0', version: '>= 1.20')

i18n = import('i18n')
gnome = import('gnome')
fs = import('fs')

cargo = find_program('cargo', required: true)

cargo_sources = files(
  'Cargo.toml',
  'Cargo.lock',
)

if get_option('profile') == 'development'
  profile = '.Devel'
  if fs.is_dir('.git')
    vcs_tag = run_command('git', 'rev-parse', '--short', 'HEAD', check: false).stdout().strip()
    if vcs_tag == ''
      version_suffix = '-devel'
    else
      version_suffix = '-@0@'.format(vcs_tag)
    endif
  else
    version_suffix = '-devel'
  endif
else
  profile = ''
  version_suffix = ''
endif

application_id = 'io.bassi.Amberol@0@'.format(profile)

subdir('data')
subdir('src')
subdir('po')

# Optional: REUSE license compliance check (skipped if reuse is not installed)
reuse = find_program('reuse', required: false)
if reuse.found()
  test('license',
    reuse,
    args: ['lint'],
    workdir: meson.project_source_root(),
    suite: ['lint'],
  )
endif

if host_machine.system() != 'windows'
  gnome.post_install(
    glib_compile_schemas: true,
    gtk_update_icon_cache: true,
    update_desktop_database: true,
  )
else
  gnome.post_install(
    glib_compile_schemas: true,
    gtk_update_icon_cache: false,
    update_desktop_database: false,
  )
endif

summary({
    'prefix': get_option('prefix'),
    'libdir': get_option('libdir'),
    'datadir': get_option('datadir'),
    'bindir': get_option('bindir'),
  },
  section: 'Directories',
)

summary({
    'Profile': get_option('profile'),
  },
  section: 'Build options',
)
