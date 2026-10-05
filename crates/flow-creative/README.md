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

- 左侧工具添加文字、导入图片、本地 3D，以及图片 / 视频 / 音频 / 3D 模型生成节点
- 导入支持 PNG、JPEG、GIF、WebP 多选；素材复制到项目，删除外部原文件不影响项目
- 图片初始尺寸保留原比例，媒体占据整个节点；调整不同纵横比时用 cover 填充
- 选中后上栏复制、分组、替换图片、设置端口色、删除；下栏编辑内容或提示词
- 文字字段停止输入后应用并自动保存；自定义 JSON 需显式校验应用
- 图形编辑、连线、分组、删除和内容编辑继续使用核心事务与 Undo/Redo
- Space+左键或中键平移；释放 Space 或鼠标会保留视口，Esc 才取消当前手势
- 端口按数据类型统一颜色；连线继承源端口类型色。可选色板或输入 `#RRGGBB`
- 仅同型 Output → Input 可连接，颜色不改变验证规则；`Any` 在 Creative 中禁用
- 顶部“数据”保留 FlowDocument/records v1 入口、自定义 Palette 示例与只读开关

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

## 四类生成节点与宿主接入

`generate_image`、`generate_video`、`generate_audio`、`generate_model` 是独立 kind，
带不同的精确输出类型。图像/视频有比例设置，视频/音频有时长设置；参数是本地请求
数据，不代表任意供应商都支持这些取值。

默认没有生成服务，生成按钮显示未连接。不会调用真实模型、计费或伪造生成结果。
宿主调用 `set_generation_available(true)` 后订阅 `CreativeEvent::GenerationRequested`，
得到含提示词、设置、输入节点和资产键的 `GenerationRequest`。连接本身不会执行任务。
宿主负责模型、凭证、权限、队列和预算，完成后通过 `apply_generation_update` 回写。
项目资产导入支持图片与下述静态 GLB；音频/视频播放仍是独立媒体接入模块。
不能把对应节点的占位符当成已生成媒体。

## 本地 3D 预览

左侧“本地3D”选择 `.glb`，或在文件选择器不可用时输入明确的本地绝对路径。
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

```sh
cargo test --locked -p gpui-flow-creative --lib
cargo test --locked -p gpui-flow-interaction --test pan_release
cargo test --locked -p gpui-flow-creative --features desktop --lib
cargo clippy --locked -p gpui-flow-creative --features desktop --all-targets -- -D warnings
```

本轮 Linux 本地已通过 Creative desktop 29 项、interaction 32 项及端口配色回归；
Creative / Flow UI / interaction 的 all-target strict Clippy、格式检查与桌面可执行文件
链接通过。Linux 原生窗口已实际检查新建、导图复制/铺满、浮层输入、拖动跟随、
Space 松开保留视口、类型颜色/自定义色/真实连线、四生成入口、返回首页、退出重开
后的资产/提示词/参数/连线/颜色/视口恢复。当前云桌面没有 XDG file chooser portal，
系统选择器仅验证了失败反馈；实际图片导入通过 UI 提供的明确本地路径入口完成。
这些结果不外推为系统文件选择器、其他平台或真实模型服务通过。

3D 增量通过 Creative desktop 53 项测试（含 17 项网格解析/栅格回归和 7 项 GLB
资产回归）、all-target strict Clippy、格式与链接。原生实际导入非对称四面体及
592 顶点 / 1120 面 / 双基色的环体与偏心球组合，确认旋转、透视遮挡、缩放、
对象拖动、切换/删除节点及完整退出重启后的资产和相机恢复。文件选择器在当前环境
仍缺 XDG portal，实际导入使用明确的本地路径入口；这些小样本不代表所有 GLB 均受支持。
