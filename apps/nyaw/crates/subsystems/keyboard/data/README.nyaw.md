xkeyboard-config 2.48

Source: https://xorg.freedesktop.org/archive/individual/data/xkeyboard-config/xkeyboard-config-2.48.tar.xz
License: COPYING and individual source file notices.

symbols, types, compat, keycodes, geometry are copied from the release. Symbol
symlinks are stored as regular copies for Windows. rules/evdev is generated using
the upstream generator (`python3 -m rules.generator rules --ruleset evdev --version
v2 --compat --output evdev rules/rules.in`). evdev.xml and evdev.extras.xml are the
release's base.xml and base.extras.xml. keycodes/evdev adds XFER/NFER aliases for
the legacy names used by symbols/jp (HENK/MUHE). No network access is needed at runtime.

Compose is libX11's nls/en_US.UTF-8/Compose.pre at revision
ff8706a5eae25b8bafce300527079f68a201d27f, with XCOMM and C comments converted
to # comments. License: COPYING-libX11.
