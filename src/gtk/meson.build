# SPDX-FileCopyrightText: 2025  Emmanuele Bassi
# SPDX-License-Identifier: GPL-3.0-or-later

blueprints = custom_target('blueprints',
  input: files(
    'playback-control.blp',
    'playlist-view.blp',
    'lyrics-view.blp',
    'queue-row.blp',
    'song-cover.blp',
    'song-details.blp',
    'window.blp',
  ),
  output: '.',
  command: [
    find_program('blueprint-compiler'),
    'batch-compile',
    '@OUTPUT@',
    '@CURRENT_SOURCE_DIR@',
    '@INPUT@',
  ],
)
