# 渐变渲染 / Gradient rendering

`gpui-flow-scene` 的 Paint 现在真正参与 GPUI 矢量填充和描边渲染：

- 任意数量色标的线性渐变
- 任意数量色标的径向渐变
- 重复 offset 的硬边色标
- 透明色标、父子 opacity、预乘 alpha 插值
- 局部坐标渐变随旋转、镜像、斜切、非等比缩放变化
- NonZero / EvenOdd 路径、圆头圆角描边，以及嵌套圆角 Frame 裁切

运行独立示例：

```bash
cargo run --locked -p flow-studio --bin gradients
```

示例覆盖四色线性、四色径向、硬边/透明度、渐变曲线描边、仿射变换和嵌套裁切。支持 SceneView 的拖动、旋转、缩放、Fit 与撤销操作。

## 为什么使用纹理渲染

检查当前锁定的 [gpui-fast Background 定义](https://github.com/longbridge/gpui-fast/blob/10d0051/crates/gpui/src/color.rs) 和 [原生 shader](https://github.com/longbridge/gpui-fast/blob/10d0051/crates/gpui_wgpu/src/shaders.wgsl) 后，确认 native Background 只公开两个色标的线性渐变，没有径向渐变。直接把场景渐变转换为它会丢失色标或改变坐标语义。

当前 adapter 没有把渐变替换成某个实色。它先沿用矢量 renderer 的本地 tessellation、仿射三角形变换与嵌套裁切，再通过纯 Rust sampler 生成设备分辨率的 BGRA 纹理，上传至 GPUI `RenderImage` / `paint_image`。实色继续走原有矢量快速路径。

纹理是透明的形状覆盖结果，不是整个场景截图。填充和描边分别绘制，图层顺序不变。覆盖掩码取三角形并集，因此共享边或重叠三角形不会重复叠加 alpha。

## 颜色与采样语义

`PaintSampler::new(&paint)` 验证 Paint；`sample(local_point)` 返回浮点直 alpha sRGB 颜色。

- 线性参数为点在 start→end 方向上的投影
- 径向参数为点到 center 的距离除以 radius
- 色标范围以外延伸最近的端点颜色
- 相同 offset 的色标保留顺序，在精确 offset 使用最后一个色标
- 在预乘 alpha 的 sRGB 分量中插值，再转换回直 alpha 输出，避免透明蓝到不透明红出现蓝色晕边
- 纹理数据为 GPUI 要求的直 alpha BGRA，不把预乘字节误传给图像管线

默认几何/裁切边缘采用 2×2 覆盖采样。完全覆盖的像素按原生 fragment shader 的方式在像素中心计算颜色。`RasterOptions.samples_per_axis` 允许 1、2、4，可选择更高的边缘采样质量。

## 变换、裁切与缩放

`rasterize_paint` 接受屏幕空间三角形，以及从图形局部空间到屏幕空间的 `Affine`。每个颜色采样点经过逆变换回到局部空间，因此非等比缩放的径向渐变自然形成椭圆，不会错误地重新适配到屏幕 AABB。

adapter 在 raster 分配前先裁切到当前窗口 content mask。放大一个图形不会按它的完整离屏尺寸分配纹理。每次缩放、旋转、裁切或屏幕像素密度变化都会重新采样，不静默降低分辨率或复用低清晰度截图。非可逆变换返回明确错误。

圆角裁切轮廓与几何曲线的离散精度随屏幕变换和设备像素比例调整。当前使用 CPU tessellation/采样，不是新增的径向 GPU shader；极大或频繁变化的复杂场景仍可能超出帧预算。

## 内存与缓存约束

默认每张纹理限制：

- 最多 4,194,304 设备像素
- 每轴最多 4096 像素
- 最多 16 MiB BGRA 结果，另有最多 8 MiB 临时覆盖掩码
- 超限返回 `GradientError::TooLarge`，不缩小后假装设备分辨率

缓存：

- UI 线程共享 LRU，最多 32 个条目、64 MiB 保留纹理字节
- key 包含窗口、图形/样式、仿射变换、裁切、设备比例、opacity 和 fill/stroke 身份
- 保持不变的帧直接复用已上传纹理
- 淘汰后的 GPU atlas 释放延迟到下一帧，避免当前帧仍引用的纹理被立即回收
- 延迟释放累计达到 64 MiB 时暂停新增 cache miss；一次最后淘汰最多再增加 16 MiB，所以保留+待释放纹理字节上限为 144 MiB，不含当前栅格化临时内存和场景几何
- 窗口提前关闭会清理待释放计数，避免后续窗口永久卡在预算限制

公开 API：

```rust
use gpui_flow::display::{
    gradient_cache_stats,
    gradient_render_issues,
    clear_gradient_cache,
};
```

`gradient_cache_stats` 提供 entries、retained_bytes、deferred_bytes、hits、misses。宿主在关闭场景等非 paint 阶段可调用 `clear_gradient_cache(window, cx)`。

## 错误反馈

`gradient_render_issues(window_id)` 是非破坏性、按窗口筛选的查询；每条包含 window_id、object 和 message。SceneView 可以继续按自己的场景 ID 筛选，多个视图不会相互消费错误。对象下一次 paint 会先清除旧诊断再重试，因此缩放/配置恢复后能清除错误。

内置 SceneView 在下一帧收集并显示这些错误，避免超限图形消失而没有说明。仍保留 `take_gradient_render_issues()` 供拥有全局诊断管理权的宿主使用；共享窗口中优先使用非破坏性查询。

文字 glyph / 图片的渐变蒙版尚未实现，仍通过 `UnsupportedFeature::GradientPaint` 明确诊断。这里的渐变支持范围是矢量 primitive 的 fill/stroke。

## 验证与性能

```bash
cargo test --locked -p gpui-flow-scene --test gradients
cargo test --locked -p gpui-flow --lib display::gradient
cargo test --locked -p gpui-flow-scene --test gradients measure_interactive_radial_raster -- --ignored --nocapture
```

测试覆盖多色标、重复 offset、透明度、BGRA 通道、逆仿射坐标、径向非等比缩放、三角形并集、覆盖抗锯齿、裁切前分配、内存/单轴限制、奇异变换、缓存 key、缓存条目/字节预算以及跨窗口诊断隔离。

一次本环境测量：500×300 径向纹理、2×2 边缘 AA、优化后的 dev 配置，首次计算约 13.8 ms，纹理 600 KB。相同测试在优化前为 52.9 ms。测量不是跨设备帧率保证；缓存命中无需重新做逐像素计算。workspace 对 `gpui-flow-scene` 的 dev 构建设置 opt-level=2，避免交互式调试构建因逐像素代码完全未优化而明显卡顿。
