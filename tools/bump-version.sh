#!/bin/sh
# Bumps the version in both places it is kept (CLAUDE.md, Versioning): Info.lua and
# klassifier/Cargo.toml. The build number always goes up by 1.
# Usage: tools/bump-version.sh [revision|minor|major]   (default: revision)
#   revision  bug fixes, docs, build tooling: 2.9.12 -> 2.9.13
#   minor     new features:                   2.9.12 -> 2.10.0
#   major     breaking changes:               2.9.12 -> 3.0.0
set -eu

cd "$(dirname "$0")/.."
info=Klassifier.lrplugin/Info.lua
cargo=klassifier/Cargo.toml
part=${1:-revision}

# "major minor revision build" from Info.lua.
current=$(sed -n 's/.*VERSION *= *{ *major *= *\([0-9]*\), *minor *= *\([0-9]*\), *revision *= *\([0-9]*\), *build *= *\([0-9]*\) *}.*/\1 \2 \3 \4/p' "$info")
if [ -z "$current" ]; then
	echo "error: no VERSION found in $info" >&2
	exit 1
fi
set -- $current
major=$1 minor=$2 revision=$3 build=$4

case $part in
	revision) revision=$((revision + 1)) ;;
	minor) minor=$((minor + 1)) revision=0 ;;
	major) major=$((major + 1)) minor=0 revision=0 ;;
	*)
		echo "usage: $0 [revision|minor|major]" >&2
		exit 2
		;;
esac
build=$((build + 1))

sed -i "s/VERSION *= *{[^}]*}/VERSION = { major = $major, minor = $minor, revision = $revision, build = $build }/" "$info"
# Only the first `version =` line: the package's (dependencies' versions are inline tables).
sed -i "0,/^version *= *\"[^\"]*\"/s//version = \"$major.$minor.$revision\"/" "$cargo"
echo "Version $major.$minor.$revision build $build"
