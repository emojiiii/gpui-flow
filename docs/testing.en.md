# Testing

[简体中文](testing.md) · [gpui-flow guide](library.md)

Run checks from the repository root. Use the Rust version pinned in the [CI workflow](../.github/workflows/ci.yml) and install the [native build dependencies](getting-started.md) for desktop code.

## Choose a test by change

| Change | Command |
| --- | --- |
| Graph model and editing | `cargo test --locked -p gpui-flow-core` |
| Runner or workflow execution | `cargo test --locked -p gpui-flow-runtime` |
| Display scene, layout, or geometry | `cargo test --locked -p gpui-flow-scene` |
| Scene text editing | `cargo test --locked -p gpui-flow-interaction --test text_tools` |
| Flow Creative projects, media, or export | `cargo test --locked -p gpui-flow-creative --features desktop --lib` |
| Runnable documentation examples | `cargo test --locked --manifest-path docs/examples/Cargo.toml` |

Pass a test name after `--lib` to reproduce a specific failure. For example, run the frame export test that checks real glyph pixels, paint outside the text box, and font changes.

```sh
cargo test --locked -p gpui-flow-creative --features desktop --lib \
  creative_native_text_keeps_ink_outside_its_layout_box_and_font_epoch
```

## Check the desktop build and documentation

```sh
cargo fmt --all -- --check
cargo build --locked -p gpui-flow-creative --features desktop --bin flow-creative
cargo clippy --locked -p gpui-flow-creative --features desktop --all-targets -- -D warnings
```

Open the desktop app on the target OS to check window input, focus, file dialogs, or visible rendering. The [CI workflow](../.github/workflows/ci.yml) lists the checks assigned to Ubuntu, macOS, and Windows. For documentation, [build and preview the site](publishing.md) before publishing.
