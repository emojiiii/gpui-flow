# gpui-flow-text

Reusable plain-text measurement contracts, independent of the Flow scene,
layout, editor and UI crates. The default build has **no dependencies**. The
optional `gpui` feature provides the existing gpui-fast native backend; it does
not implement another font shaper or install fonts.

Use the package from this repository (it is not published to crates.io):

```toml
# Contracts only; no GPUI dependency.
gpui-flow-text = { path = "../gpui-flow/crates/flow-text" }
# Or enable the native implementation with the same pinned gpui-fast version:
# gpui-flow-text = { path = "../gpui-flow/crates/flow-text", features = ["gpui"] }
```

```rust
use gpui_flow_text::TextMeasureRequest;
let request = TextMeasureRequest {
    content: "Hello, 画布 👩‍💻".into(),
    font_family: "sans-serif".into(),
    font_size: 16.0,
    font_weight: 400,
    line_height: 20.0,
    wrap_width: Some(200.0),
};
request.validate().unwrap();
```

A host can implement `TextMeasurer`, or enable `features = ["gpui"]` and construct
`GpuiTextMeasurer::new(app.text_system().clone())`. `measure` uses the actual
GPUI `WindowTextSystem::shape_text` and `WrappedLine::size`. For painting with
matching font and wrapping policy, `gpui_backend::shape_text` returns GPUI's
`WrappedLine` values directly. Use the same app text system for both.

All sizes are logical model units. Empty text has one line; a final newline
preserves an additional empty line. Family, weight, font size, absolute line
height and optional wrap width determine the result. Platform fallback, CJK,
emoji and shaping remain GPUI's responsibility. Metrics use line boxes, not
ink bounds. Errors return without replacing document geometry.

The native adapter bounds its successful-result cache to 128 entries / 1 MiB.
Call `invalidate` when host font resources change. An invalidation interleaved
with shaping rejects the stale measurement. Host font registration and GPUI's
own resource caches are the host's responsibility.

Existing users can keep `gpui_flow_layout::{TextMeasureRequest, TextMetrics,
TextMeasurer}` and `gpui_flow::GpuiTextMeasurer`: these re-export the same types.
`FlowLayout`, `TextSizing`, reactive bindings and Undo integration stay in
`gpui-flow-layout`; the measurement crate does not own document transactions.

Verification:

```sh
cargo test --locked -p gpui-flow-text --no-default-features
cargo test --locked -p gpui-flow-text --features gpui
cargo run --locked -p flow-studio --bin text_measure_probe
```
