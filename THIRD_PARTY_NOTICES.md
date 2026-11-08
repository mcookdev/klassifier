# Third-party notices

Klassifier's own code is under the MIT license (see `LICENSE`). A built and
deployed copy also contains or uses the components below, each under its own
license. Full license texts are in the `licenses/` folder.

## SigLIP model

- Files: `models/siglip/vision_model.onnx`, `models/siglip/text_model.onnx`,
  `models/siglip/tokenizer.json`
- Source: SigLIP base (`google/siglip-base-patch16-224`), by Google
  <https://huggingface.co/google/siglip-base-patch16-224>
- License: Apache License 2.0 (`licenses/Apache-2.0.txt`)
- Changes: Google's original weights were converted to ONNX by Xenova
  (`Xenova/siglip-base-patch16-224`, revision
  `4649052661e53c7000355844105f8a1792088239`). Klassifier uses those ONNX files
  unmodified. This project is not affiliated with or endorsed by Google.

## rawler

- Compiled into `klassifier` (`klassifier.exe` on Windows; reads RAW files)
- Source: <https://github.com/dnglab/dnglab>, version 0.8.0, by Daniel
  Vogelbacher, Pedro Côrte-Real and contributors
- License: GNU Lesser General Public License 2.1 (`licenses/LGPL-2.1.txt`)
- Klassifier links rawler statically. To use a modified rawler, change the
  `rawler` dependency in `klassifier/Cargo.toml` to point at it and rebuild
  with `make build`. Klassifier's full source is in this repository.

## ONNX Runtime

- Files: `onnxruntime.dll`, `onnxruntime_providers_shared.dll` (Windows);
  `libonnxruntime.dylib` (macOS)
- Source: <https://github.com/microsoft/onnxruntime>. Windows:
  `Microsoft.ML.OnnxRuntime.DirectML` 1.24.4 from NuGet. macOS:
  `onnxruntime-osx-arm64-1.24.4.tgz` from its GitHub release, which also
  contains WebGPU support (Dawn, under BSD-3-Clause, among others). The
  licenses of what it contains are in `onnxruntime-ThirdPartyNotices.txt`,
  deployed next to it
- License: MIT, Copyright (c) Microsoft Corporation
  (`licenses/onnxruntime-MIT.txt`)

## DirectML (Windows only)

- File: `DirectML.dll`
- Source: `Microsoft.AI.DirectML` 1.15.4 from NuGet
- License: Microsoft Software License Terms for DirectML
  (`licenses/DirectML.txt`), which allow redistributing it as part of an
  application on Windows

## Slint

- Compiled into `klassifier-ui` (`klassifier-ui.exe` on Windows,
  `Klassifier.app` on macOS; the plugin's windows)
- Source: <https://github.com/slint-ui/slint>, version 1.18.1, by SixtyFPS GmbH
- License: Slint Royalty-free Desktop, Mobile, and Web Applications License
  2.0 (`licenses/Slint-Royalty-free-2.0.md`). Slint is also available under
  GPL-3.0; Klassifier uses the royalty-free license. As it requires, the
  review window's About (ⓘ) button shows Slint's `AboutSlint` attribution.

## Rust crates

`klassifier` and `klassifier-ui` also include Rust crates from crates.io. Almost all of them
are under MIT and/or Apache-2.0, and some use other permissive licenses (BSD,
Zlib, ISC, Unicode-3.0). Run `cargo tree` in `klassifier/` to list them; each
crate's license is in its `Cargo.toml` on crates.io.
