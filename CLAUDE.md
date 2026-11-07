# Project rules

## Versioning

The version is defined in two places that must stay in sync:
- `Klassifier.lrplugin/Info.lua`: `VERSION = { major, minor, revision, build }`
- `klassifier/Cargo.toml`: `version = "major.minor.revision"`

With every build (any change to the plugin, the app, docs or build tooling), and before
*every* `make deploy`, including debug and experiment builds that won't be committed:
- Increment `build` by 1. It never resets.
- Bump at least `revision`, so every build has a new semver version. Bump the part that
  matches the change: `major` for breaking changes, `minor` for new features, `revision` for
  bug fixes and everything else (docs, build tooling). Reset the lower parts to 0 when
  bumping a higher one.
- `make deploy` enforces this (`tools/check-version.sh`): it fails if `Info.lua` and
  `Cargo.toml` disagree, or the version and build aren't both newer than the deployed
  ones. Bump the version; never work around the check.
- This is a personal project, no PR are necessary.