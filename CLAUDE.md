# Project rules

## Versioning

The version is defined in two places that must stay in sync:
- `HelloWorld.lrplugin/Info.lua`: `VERSION = { major, minor, revision, build }`
- `hello-app/Cargo.toml`: `version = "major.minor.revision"`

With every change to the plugin or the app:
- Increment `build` by 1. It never resets.
- Bump the semver part that matches the change: `major` for breaking changes, `minor` for new
  features, `revision` for bug fixes. Reset the lower parts to 0 when bumping a higher one.
- Changes that are only docs or build tooling increment `build` only.
