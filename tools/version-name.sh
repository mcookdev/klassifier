#!/bin/sh
# Prints the version for file names, from Info.lua: 3.2.0-build64. CI names the plugin's zip
# with it (.github/workflows/build.yml).
set -eu

cd "$(dirname "$0")/.."
sed -n 's/.*VERSION *= *{ *major *= *\([0-9]*\), *minor *= *\([0-9]*\), *revision *= *\([0-9]*\), *build *= *\([0-9]*\) *}.*/\1.\2.\3-build\4/p' \
	Klassifier.lrplugin/Info.lua
