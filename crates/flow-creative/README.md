# Flow Creative

原生、本地优先的创作工作台。复用现有 `FlowCanvas`、`Graph`、端口验证与 `Editor`，
没有替换底层画布引擎或改变 Leafer compatibility profile。

交互参考 Lovart 的公开[画布对象](https://www.lovart.ai/docs/edit-your-design/canvas-objects)
与[画布操作](https://www.lovart.ai/docs/edit-your-design/customize-your-canvas)：
媒体直接铺在画布上，选中对象时显示跟随移动的上下工具。类型端口、本地项目与可连接
生成节点是本项目的扩展，不声称逐像素复刻或使用 Lovart 内部模型。

## 启动

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative
# 指定本地项目目录
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --projects-dir ./projects
# 旧 FlowDocument 独立打开；示例必须显式启动
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --file ./board.flow.json
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --demo
```

启动先进入项目首页。新建项目先在磁盘创建空项目，再打开空白画布。默认位置：
- Linux：`$XDG_DATA_HOME/gpui-flow/projects`，或 `~/.local/share/gpui-flow/projects`
- macOS：`~/Library/Application Support/Flow Creative/Projects`
- Windows：`%LOCALAPPDATA%/Flow Creative/Projects`

## 操作

- 左侧图标工具选择/平移画布、导入图片、添加文字与四类生成节点；悬停查看名称
- 点击左上项目名重命名；首页使用项目内图片作封面，改名和当前画布一起原子保存
- 导入支持图片、音视频和 GLB 多选；可从文件管理器拖到画布落点，素材复制到项目
- 图片初始尺寸保留原比例，媒体占据整个节点；调整不同纵横比时用 cover 填充
- 选中图片后上栏裁剪/旋转/翻转、替换、图生图、恢复原比例、复制/编组/端口色/删除；下方描述入口默认收起
- 图片节点连接图片生成器的图片输入即图生图；无图片输入时为文生图。图片上栏“图生图”可直接创建已连好的生成器，断开边回到文生图
- 生成器提示框仅展示已连接图片输入的缩略图，不单独选择/保存附件；图片导入画布后再连线。视频/模型节点的图片输入保留各自含义，不称为图生图
- 左侧文件夹打开“本项目素材”，复用图片、音视频或GLB到画布；复用不会重复复制文件，图库不会隐式给生成器挂参考图
- 素材弹层每页 8 项，图片在后台生成缩略图并显示加载/失败状态；缓存最多 32 项。音视频与模型显示类型标记，Escape 关闭弹层
- 文字字段停止输入后自动保存；画布手势期间保留草稿，结束后应用。自定义 JSON 需显式校验
- 文字对象透明显示在画布上；双击或点“编辑文字”就地输入，Escape/“完成编辑”结束。上栏调整 12–144px 字号、粗体和对齐，样式随项目保存并支持撤销
- 原生多行输入器编辑时左对齐；选对齐按钮会完成编辑并立即显示对象排版
- 图层按钮弹出对象列表并切换选择；浮栏跟随对象，尺寸保持屏幕像素
- 多选对象共用轻量工具条，可复制、编组或删除；单选/多选工具条在画布四边保持可见
- “适应画布”为对象外标题与浮栏预留空间；顶部项目/撤销工具组有浅底色，平移经过图片时仍清晰可读
- 图形编辑、连线、分组、删除和内容编辑继续使用核心事务与 Undo/Redo
- Space+左键或中键平移；释放 Space 或鼠标会保留视口，Esc 才取消当前手势
- 端口按数据类型统一颜色；连线继承源端口类型色。可选色板或输入 `#RRGGBB`
- 仅同型 Output → Input 可连接，颜色不改变验证规则；`Any` 在 Creative 中禁用
- 右上“…”项目选项保留保存、GLB 导入、FlowDocument/records v1、自定义 Palette 与只读开关

## 本地持久化

`ProjectStore` / `LocalProject` 是无 GPUI 的模型 API。版本化 `project.json` 将元数据、
未改变格式的 `FlowDocument` 与资产 manifest 放在同一个原子替换事务中。视口、节点、
连线、Frame、节点数据和 `graph.data.port_colors` 均随项目保存。导入/保存失败保留旧稿；
旧 handle 的 revision 冲突拒绝覆盖。后台保存和素材导入由 UI 串行化，回首页等待保存。
正常窗口关闭会先保存最后的编辑；强制终止进程仍可能丢失最后一次未完成写入。

每个项目的素材键仅形如 `assets/a-….png`。任意 JSON URL、绝对路径、`..`、symlink
不自动转成文件读取；不上传图片，不保存凭证。SVG 仅允许宿主显式注册的内置资源，
本地导入不接受 SVG。单图 32 MiB / 64 MP，单项目最多 512 素材 / 512 MiB。
图像头/尺寸在导入时检查，完整像素解码由 GPUI 完成。磁盘原子 rename 的耐久保证
最终取决于平台和文件系统；不把可信宿主扩展包装成恶意并发进程安全沙箱。

## 图片裁剪、旋转与翻转

选中本地静态 PNG/JPEG 图片，点上栏裁剪图标。图片上方选择自由/预设比例或输入 W/H
像素，Enter 更新裁剪框；拖动四角或框内调整区域。下栏左右旋转 90°、水平/垂直翻转，
预览按当前方向显示原图与裁剪网格。Ctrl/Cmd+Z 在草稿内撤销，Shift+Z 重做；Esc 取消，
取消不写入新资产。当前不支持任意角度旋转、动画图片或调色。

“应用”在后台生成新 PNG 并复制入项目，替换当前图片对象并同步几何；原图保留在本项目
素材中。一次 Undo/Redo 同时恢复图片和尺寸，结果随项目重开恢复。更换图片、锁定对象、
切换选择或关闭项目会作废草稿，异步任务核对目标身份后才回写。下游 Mask 和处理输出
继续按源资产身份检查有效性。处理限制：单边 16384、16 MP、输入/输出 32 MiB、解码
128 MiB、60 秒截止；预览最长边 768，快速旋转合并到最新状态，不排无限任务。

## 四类生成节点与宿主接入

`generate_image`、`generate_video`、`generate_audio`、`generate_model` 是独立 kind，
带不同的精确输出类型。图像/视频有比例设置，视频/音频有时长设置；参数是本地请求
数据，不代表任意供应商都支持这些取值。

默认没有生成服务，生成按钮显示未连接。不会调用真实模型、计费或伪造生成结果。
宿主调用 `set_generation_available(true)` 后订阅 `CreativeEvent::GenerationRequested`，
得到含提示词、设置、输入节点和资产键的 `GenerationRequest`。连接本身不会执行任务。
宿主负责模型、凭证、权限、队列和预算，完成后通过 `apply_generation_update` 回写。
图片来源仅在 `GenerationRequest.inputs` 中，兼容原有 `reference` 端口 ID；宿主通过
当前 `LocalProject` 解析其中资产键。`settings.reference_asset` 不再是有效请求来源。
打开旧本地项目时，已存在的独立图片附件转换为可见图片节点及类型匹配的输入边，复用
项目内文件；已存在同素材的图片节点会优先复用。转换幂等并在验证后原子保存。若附件
与已有图片输入冲突、资产缺失或验证失败，则保留原项目并明确报错，不覆盖合法连线。
宿主返回已导入的音视频资产键时，对应生成节点也显示真实媒体。不能把未接服务的
占位符当成已生成媒体。

## 本地音视频对象

左侧导入按钮、文件拖放和路径入口支持 MP4/WebM 视频、WAV/MP3/FLAC/OGG/M4A 音频。
文件复制到项目后由已有 FFmpeg 后端验证，失败不会添加资产；播放仅使用项目资产键。
视频首帧和后续帧直接显示在节点内，选中后下方浮栏播放/暂停、前后跳转 5 秒和静音。
切换到其他对象时暂停播放，删除或关闭项目会回收解码进程与帧缓存。播放位置是会话
状态，项目重开从首帧开始；资产、对象位置与尺寸会持久化。

需要系统已安装 `ffmpeg`、`ffprobe` 和用于无窗口音频输出的 `ffplay`；缺失时显示实际
错误。不会自动安装工具、读取播放列表或访问远程 URL。单媒体 128 MiB / 30 分钟，
源视频最多 4096×4096 总像素，预览至多 640×360 / 24fps，同时缓存 4 个媒体对象；
选中其他对象会按需切换缓存。原始素材不会重编码。音频卡片显示真实时长，不伪造波形。
这是预览播放器，沿用现有后端的时钟，不承诺音频采样级同步。

## 自定义节点输入与输出

左侧处理菜单 → “自定义节点”打开声明编辑器，可修改名称、接口 ID，增删输入/输出并选择
文字、数值、布尔、对象、Image、Video、Audio、Model、Mask 类型。端口 ID 稳定且不复用，
同名标签不会混淆连接；最多16端口，至少一个输出。声明随项目保存、重开、JSON/records
导入导出，仍由原有 Flow 命令、类型连接与 Undo/Redo 处理。

内置 `local.pass_through` 是真实本地接口：输入/输出按声明顺序一一匹配，返回同类型值或
现有项目媒体引用，不复制原图、不调用模型。每个输入必须明确连接；创建时可把所选对象
连到第一个匹配输入。未注册的接口可保存设计，但“运行接口”禁用并说明尚未接入。

宿主可通过 `CreativeCanvas::register_custom_executor` 注册 `CustomNodeExecutor`。UI 在后台
执行一次显式请求，宿主必须合作响应取消；关闭会请求取消并等待宿主返回。模板只保存
注册 ID 和数据，不执行任意脚本、命令或 URL，也不持久化凭证。结果必须按所有声明输出
端口返回精确类型；媒体键必须已属于当前项目，Mask 还校验专用类型、原图身份和尺寸。

完成结果保留输入快照。源端口、输入值或参数变化后旧输出不能继续流入生成、处理或Mask
节点；异步结果也检查当前项目、节点及请求。单 Image/Text 输出可在自定义节点中预览；
其他/多输出显示真实就绪状态，通过端口继续消费。最多64种项目类型，声明/请求/结果各有
64 KiB及嵌套/项数预算，依赖循环会被拒绝，不隐式运行上游。

## 本地媒体处理节点

图片/视频/音频对象上栏的处理按钮创建一个已连接输入的处理节点；左侧“本地媒体处理”
也可添加独立节点再连线。所有节点使用显式 `input` / `output` 类型端口，可继续连接其他
处理节点或生成节点。点“运行本地处理”才执行，参数变化或输入变更后必须重新运行。
成功输出是项目内的新素材，原素材始终保留；失败和取消不会宣称完成或替换已有有效结果。

- 图片：静态 PNG/JPEG，PNG 保留透明，JPEG 透明填白；质量、等比最长边，不放大
- 视频：本地 FFmpeg 转 MP4/H.264，调整 CRF/最长边，保留所有音轨并转 AAC
- 音频：首条音轨转 WAV/PCM16 或 MP3，可选采样率及 MP3 码率

任务在后台串行执行，运行时可取消，关闭窗口会取消并等子进程回收。新文件验证成功后才
复制入项目，并检查目标节点、输入与参数仍对应本次请求；临时输出自动清理。图片最多
32 MiB/16 MP、最长边1–8192；音视频最多128 MiB/30分钟，视频最长边2–4096，任务最长
5分钟。图片解码/缩放/编码步骤之间合作取消；FFmpeg 子进程取消会 kill/wait。需要系统已有
FFmpeg/FFprobe，缺失时显示错误，不自动安装，不接受任意命令、滤镜脚本或远程地址。

## 独立 Mask 节点

图片上栏的画笔按钮创建已连接原图的 Mask 节点，也可从左侧添加后手动连接 Image 输入。
当前编辑静态 PNG/JPEG 项目图片。点“绘制遮罩”进入画笔/橡皮模式，每笔单独撤销/重做；
Escape 取消未完成的笔画并退出。普通状态仍拖动节点。改变节点纵横比时，笔画按原图
在节点中的等比内容区域定位，不绘入留白。原图与笔画分别保存，原图不会被覆盖。

点“应用遮罩”在后台生成与原图尺寸完全相同的 8 位灰度 PNG，内部约定 0 保留、255 编辑。
这是独立的 `Named("mask")` 输出与专用资产类型，不把普通图片自动转换成 Mask；具体生成
服务的黑白或 alpha 约定由宿主适配器转换。源图片资产身份或尺寸变化会提示重置，旧笔画
保留到用户明确重置；未应用或过期的输出不能用于图片编辑请求。

“用于图片编辑”建立 `edit_image` 节点，分别连接 Image 和 Mask 输入；仍需宿主明确接入
生成服务，应用 Mask 本身不会调用模型。`GenerationRequest.kind` 为 Image，输入端口为
`image` / `mask`，源资产必须一致；输出结果沿用现有宿主更新接口。

每个遮罩最多 128 笔 / 2048 点、源图 64 MP，栅格工作量有界且支持取消；预览最长边512px，
最多缓存8个视图。笔画是硬边圆头，当前不含羽化。预览、导出和项目保存分开，旧任务按
节点内容及原图身份检查后接收，输出保存仍使用项目原子事务。

## 本地 3D 预览

拖入 `.glb`，或从右上项目选项/模型生成器下方选择“导入GLB”；文件选择器不可用时可输入明确的本地绝对路径。
`LocalProject::import_model` 在后台验证后复制文件到项目；模型节点输出类型与
`generate_model` 一致。选中节点后，下方按钮旋转、俯仰、缩放或复位视角；拖动画面
仍移动节点。相机状态随项目保存，并支持 Undo/Redo。宿主也可将已导入 GLB 资产键
传给 `apply_generation_update`，在模型生成节点中显示真实几何。

这是 CPU 静态网格预览：使用 `gltf 1.4.1` 解析内嵌 BIN 的 GLB 2.0，支持节点变换、
网格实例、三角形索引/非索引、透视裁剪、深度遮挡与不透明 baseColorFactor/平面光照。
只显示默认或首个场景；不还原纹理、PBR、顶点色、原始法线或场景相机。不支持动画、
骨骼、morph、压缩扩展、透明材质和非三角形图元。所有外部/data URI 均拒绝，
不会自动读取关联文件或联网获取纹理；不支持独立 `.gltf` 文件。

单文件 32 MiB、JSON 4 MiB、10 万顶点、5 万三角面；另有节点深度/实例与栅格采样预算。
原生预览固定 512×384，每个工作台最多缓存 12 个模型视图。后台解析/栅格任务串行，
每视图合并连续相机请求，旧请求不会覆盖新视角，删除/退出后丢弃结果并释放图像缓存。
`resolve_asset_path` 仅验证 manifest/路径/文件大小，供 UI 后台解码使用；需要完整验证
的宿主使用 `resolve_asset`。两者均只解析项目已有资产键。

## 可复用 API 与验证

`CreativeCanvas::new`、`CreativeRegistry`、`NodeDefinition`、原有 records 映射和
`NodeRenderer` 接口保留。`NodeRenderer::full_bleed()` 默认为 false，老渲染器不变；
新媒体渲染器可选择裸对象展示。`FlowCanvas::set_host_persistence(true)` 让保存快捷键
发出 `FlowEvent::SaveRequested`，默认独立画布仍走原有文件保存。

`NodeRenderer::transparent_background(&Node)` 默认 false；Creative 文字选择透明背景。
`TextStyle` / `edit_text_style` 验证 `data.style` 并保留宿主字段。`CreativeCanvas::set_assets`
安装标准媒体/文字渲染器与就地编辑绑定，使用自定义渲染器的宿主仍可保留自己的 registry。

```sh
cargo test --locked -p gpui-flow-creative --lib
cargo test --locked -p gpui-flow-interaction --test pan_release
cargo test --locked -p gpui-flow-creative --features desktop --lib
cargo clippy --locked -p gpui-flow-creative --features desktop --all-targets -- -D warnings
```

本地验证覆盖项目/资产/改名、草稿保存、类型连线、GLB解析/栅格，以及旧演示迁出的
共享阴影预算和GIF回归。原生检查真实图片文件拖入、上下浮栏、参考连线、导航和项目存取。
音视频验证包含真实 WAV 复制/损坏导入清理，以及画布内 MP4 首帧、播放时钟、暂停、跳转、
快速切节点和项目重开。云端音频通过 SDL dummy 验证输出进程与回收，未验证实际听感。
当前Linux桌面缺 XDG file chooser portal，系统选择器仅确认失败反馈；路径入口和真实
文件拖放独立验证。其他平台、实际生成服务和任意GLB不由这些检查保证。
