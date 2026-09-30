# 本地音视频解码与播放

`gpui-flow-media` 是不依赖 GPUI 的播放后端；`gpui_flow::MediaView` 是可独立使用的 GPUI 播放器。音频和视频节点通过 `.player(Entity<MediaView>)` 组合它，不再只有元数据占位。

当前后端使用宿主安装的 **ffmpeg + ffprobe + ffplay**：

- ffprobe 在线程中读取真实时长、视频尺寸和音频流元数据
- ffmpeg 解码、缩放和重采样视频帧，输出 BGRA 到有界管道/队列
- GPUI 将 BGRA 帧上传为 RenderImage；替换和销毁时显式清理旧纹理
- ffplay 使用原生 SDL 音频设备输出真实音频，包括视频文件中的音轨
- Play、Pause、Stop、时间轴点击 seek、±5 秒、位置/状态/错误均可操作
- `set_volume(0..=1)` 由 controller 提供，界面可 Mute / 恢复25%；默认 25% 音量，不自动播放
- Pause/seek/Stop/Drop 会停止并回收对应的解码器和音频进程；暂停的 seek 只解码一张预览帧

## 依赖发现与信任

从 [FFmpeg 官方下载页](https://ffmpeg.org/download.html) 选择适合系统的发行包，确保三个可执行文件都存在。不同发行包包含的编解码器和许可可能不同；集成方应自行审查分发许可。库不会下载或安装程序。

`FfmpegTools::default()` 从宿主 PATH 查找 `ffmpeg`、`ffprobe`、`ffplay`。生产程序推荐由可信配置指定绝对路径：

```rust
use gpui_flow::media_backend::{FfmpegBackend, FfmpegTools};
let backend = FfmpegBackend {
    tools: FfmpegTools {
        ffmpeg: "/trusted/bin/ffmpeg".into(),
        ffprobe: "/trusted/bin/ffprobe".into(),
        ffplay: "/trusted/bin/ffplay".into(),
    },
    ..Default::default()
};
// availability() 会启动有超时的 -version 检查，应放到后台线程
```

工具路径属于宿主配置，不能从节点 JSON、供应商结果或导入文档读取。节点数据只携带本地媒体路径。所有进程调用都用 `Command::new(program).args(...)`，不使用 shell，不把媒体数据拼成命令。

Linux 需要可用的音频服务/设备；macOS 和 Windows 需要具有 SDL 音频输出的 FFplay 发行包。无设备、缺少工具、损坏文件或解码失败会显示明确错误，不会悄悄降级成“播放成功”。当前已在 Linux FFmpeg 7.1.5 验证；macOS/Windows 代码采用相同跨平台进程方式，但仍需在目标机器验收。

## 最小集成

```rust,ignore
use gpui_flow::{MediaView, VideoNode};
use gpui_flow::media_backend::{FfmpegBackend, MediaBackend, PlaybackOptions};

let controller = FfmpegBackend::default().open(
    std::path::Path::new("/media/clip.mkv"),
    PlaybackOptions::default(),
)?;
let player = cx.new(|cx| MediaView::new(controller, cx));
let body = VideoNode::new("/media/clip.mkv").player(player);
```

`MediaBackend` / `MediaController` 是可替换接口。其他后端可实现同一控制器，而不更改节点和 GPUI 适配层。`MediaView` 持有的控制器视为该视图所有；视图释放时会调用 shutdown，不应让多个独立视图意外共享同一个有不同生命周期的控制器。

`MediaController::snapshot()` 返回廉价的快照：帧像素使用 Arc，共享不会复制整帧。字段包括 state、position、metadata、frame、volume、audio_output、error、revision 和诊断用 process_ids。命令通过 32 槽有界队列提交，队列满会返回错误，不会无限堆积 seek。

`FlowCanvas` 的普通构造默认不启用媒体后端；宿主显式设置 `media_backend` 后，非空的本地 audio/video source 会创建播放器。离线 Studio 示例启用了默认后端，空节点仍显示可编辑占位；修改 source、删除节点、加载另一个文档会释放旧播放器。

## 可复现的原创 fixture 与示例

不下载外部视频，完全在本机生成测试图和低振幅正弦波：

```bash
python3 scripts/generate-media-fixtures.py
cargo run --locked -p flow-studio --bin media
```

默认生成 `target/media-fixtures/test-pattern.mkv` 和 `tone.wav`。示例不会自动播放；点击 Play 后才输出音频。可传入自己的两个本地文件：

```bash
cargo run --locked -p flow-studio --bin media -- /path/video.mkv /path/audio.wav
```

只验证视频、不打开音频设备：

```bash
cargo run --locked -p flow-studio --bin media -- --muted
```

这里的 `--muted` 使用 `AudioOutput::Disabled`，明确不创建音频输出进程。它不能证明真实音频设备可用。

## 精度与当前边界

这是编辑器预览播放器，**不是采样级同步的专业播放器**：

- 视频和音频从同一个 seek 位置启动，显示位置由同一个单调时钟计算
- 视频帧按固定预览帧率调度，默认 24fps；最多保留两个排队帧，并丢弃已经过时的显示帧
- FFplay 使用自己的设备时钟；进程启动、解码、音频缓冲和文件 seek 精度会造成可见/可听偏差，没有承诺固定毫秒级 A/V 误差
- 暂停、seek 和播放中的音量变化通过停止/重启进程实现，会有短暂空隙；不是无缝播放，也不支持变速、循环或多轨混音
- 文件的有限时长必须可探测；不支持网络 URL、直播、播放列表、字幕渲染或受保护媒体
- 预览最大尺寸和帧率受限，不是原始高分辨率编辑输出；颜色转换使用 FFmpeg 默认行为，没有完整的 HDR/色彩管理保证

## 资源和输入限制

默认单个播放器限制：输入最大 1GiB、原始视频最多 4096×4096 像素、时长最多 4 小时、探测 10 秒超时、解码无进展 10 秒超时。预览默认 640×360 / 24fps，可配置上限 1920×1080 / 60fps。FFmpeg 单次分配上限 64MiB，解码/输出线程数限制为 2；这些不能替代操作系统级总内存/CPU 沙箱。

只接受可 canonicalize 的普通本地文件；允许的封装为 mov/mp4、matroska/webm、avi、wav、flac、mp3、ogg、aac、flv、mpeg、mpegts。协议白名单禁用网络，封装白名单排除播放列表。错误日志和探测输出有大小上限。FFmpeg 仍是处理不可信二进制的外部解析器，应保持安全更新。

队列和预览尺寸是每个播放器的限制；宿主仍需限制同时打开的播放器总数，避免大量节点同时消耗资源。

## 检查与验收

无需 FFmpeg 的测试：

```bash
cargo test --locked -p gpui-flow-media
```

真实 FFmpeg 解码/生命周期测试默认标记 ignored，避免在缺少工具的机器上假装通过。安装工具后明确运行：

```bash
SDL_AUDIODRIVER=dummy cargo test --locked -p gpui-flow-media -- --ignored --test-threads=1
```

该命令覆盖真实视频解码、时间变化、暂停、seek 预览、停止、终态后重播、损坏输入、进程回收、立即销毁，以及真实 FFplay 音频输出代码路径。SDL dummy 是测试输出设备，**不证明扬声器实际可听**。真实设备验收应不设置 dummy，手动检查声音、A/V 偏差、快速连续 seek、节点删除与窗口关闭后的资源释放。

官方接口参考：[FFmpeg 命令行](https://ffmpeg.org/ffmpeg.html)、[本地/管道协议与白名单](https://ffmpeg.org/ffmpeg-protocols.html)、[FFplay 音频与 seek 选项](https://ffmpeg.org/ffplay.html)
