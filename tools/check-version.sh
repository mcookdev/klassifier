#!/bin/sh
# Run by `make deploy`: refuses to deploy unless the version in Info.lua matches
# Cargo.toml and is newer than the last successful deploy (see CLAUDE.md, Versioning).
# Usage: check-version.sh <plugin dir> <deployed plugin dir>
#        check-version.sh --stamp <plugin dir> <deployed plugin dir>
# --stamp records the version as deployed, in <deployed plugin dir>/.deployed-version.
# `make deploy` runs it after copying, so a deploy that fails partway can be retried.
set -eu

stamp=false
if [ "${1:-}" = --stamp ]; then
	stamp=true
	shift
fi

# "major.minor.revision build" from an Info.lua, or nothing.
lua_version() {
	[ -f "$1/Info.lua" ] || return 0
	sed -n 's/.*VERSION *= *{ *major *= *\([0-9]*\), *minor *= *\([0-9]*\), *revision *= *\([0-9]*\), *build *= *\([0-9]*\) *}.*/\1.\2.\3 \4/p' "$1/Info.lua"
}

here=$(lua_version "$1")
cargo=$(sed -n 's/^version *= *"\(.*\)"/\1/p' klassifier/Cargo.toml | head -n 1)
semver=${here% *}
build=${here#* }

if [ -z "$here" ]; then
	echo "error: no VERSION found in $1/Info.lua" >&2
	exit 1
fi
if [ "$semver" != "$cargo" ]; then
	echo "error: Info.lua is $semver but klassifier/Cargo.toml is $cargo; keep them in sync" >&2
	exit 1
fi

if $stamp; then
	echo "$here" > "$2/.deployed-version"
	exit 0
fi

deployed=$(cat "$2/.deployed-version" 2>/dev/null || true)
if [ -n "$deployed" ]; then
	old_semver=${deployed% *}
	old_build=${deployed#* }
	newest=$(printf '%s\n%s\n' "$old_semver" "$semver" | sort -V | tail -n 1)
	if [ "$build" -le "$old_build" ] || [ "$semver" = "$old_semver" ] || [ "$newest" != "$semver" ]; then
		echo "error: version $semver build $build is not newer than the deployed $old_semver build $old_build." >&2
		echo "Every build gets build + 1 and at least a new revision (CLAUDE.md, Versioning)." >&2
		exit 1
	fi
fi
echo "Version $semver build $build"
