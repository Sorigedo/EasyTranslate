# EasyTranslate

Rust 编写的 macOS 菜单栏翻译工具。按快捷键打开右上角翻译窗口，在窗口中输入或粘贴文字，译文会自动更新；确认后复制译文，再粘贴到微信、WhatsApp 等聊天输入框。主流程不读取、定位或自动发送聊天内容。

## 翻译服务

新安装默认选择本地 Firefox Translations Marian，联网授权初始关闭。翻译页面的服务菜单包含本地 Marian、谷歌翻译、LibreTranslate 和 OpenAI Codex CLI；在设置中刷新 Ollama 后，每个已安装模型也会作为独立选项加入菜单。

本地 Marian（由 `fxtranslate` 驱动）在本机运行，原文不会发送到翻译服务，也没有日/月字符额度或账号要求。每个语言方向首次使用时从 Mozilla Remote Settings/CDN 下载一次并缓存到：

```text
~/Library/Application Support/EasyTranslate/Models
```

这不是每次翻译都下载语言包：已缓存的语言方向可离线使用；新增语言才会按需准备对应模型。当前实测中译英模型约 52.0 MiB，英译中约 49.5 MiB，首次冷启动约 25 秒，模型载入后单句约 40–80 ms。首次下载需要网络，失败会在窗口中明确显示，不会伪造译文或自动切换服务。

选择“谷歌翻译”会调用 Google 网页翻译接口，需要打开“启用联网翻译”，不需要 API Key。接口可能受 Google 限流或变更影响。

可在“设置 → 服务设置”选择 LibreTranslate，填写你部署的 LibreTranslate API 地址和实例要求的可选 API Key，保存后即可在翻译卡片的“更换”菜单选择它。地址可填服务根地址（例如 `http://localhost:5000`），程序会请求 `/translate`。LibreTranslate 是开源软件，但公共服务器可能需要 Key、限流或不可用；EasyTranslate 不提供或默认信任公共实例。启用联网翻译后，原文会发送到你填写的服务器。实例需安装所选语言方向的翻译模型；简体中文请求会使用 `zh` 语言代码。

选择“OpenAI（本地 CLI）”会启动本机 `codex exec` 子进程。请先安装并登录 Codex CLI；设置中的“Codex 模型”下拉可选择常用模型，也可以选择“跟随本机 Codex 配置”。翻译使用临时会话和低推理档，减少启动和思考开销；通常 `gpt-6-sol` 会比高推理模型更快。发送原文前需要打开联网授权；CLI 不可用时会在状态提示中说明原因。图形界面启动时会从标准 Homebrew 和用户 CLI 目录查找 Codex，也会向子进程传递代理环境变量。

默认 `Option+D` 全局开关翻译窗口。Google、LibreTranslate 及在线模型优先使用 `HTTP_PROXY`、`HTTPS_PROXY` 或 `ALL_PROXY` 环境变量；未设置时，macOS 会自动读取系统 HTTP/HTTPS 代理（例如 VPN 客户端提供的 `127.0.0.1:7890`）。

使用本地聊天模型时，请先启动 Ollama。设置页选择一个 Ollama 服务后，可以点击“浏览 Ollama 模型库”查找模型，把模型名（如 `qwen2.5:3b`、`hf.co/...`）或模型网页地址粘贴到“模型名 / 地址”并点击“下载并添加”；下载完成后会自动出现在翻译页面菜单。选中要移除的 Ollama 模型后点击“删除当前模型”，确认后会同时删除本机模型文件。也可以点击“刷新 Ollama 模型”同步现有安装。应用请求 `http://127.0.0.1:11434` 的原生 API，也可用 `OLLAMA_HOST` 指定地址。Ollama 模型在本机运行，不需要联网授权。连接失败时会显示错误，不会自动切换服务。

旧版本中保存的 SiliconFlow 配置仍可继续使用。该服务需要 API Key 和联网授权，原文会发送到配置的远程端点；SiliconFlow 失败时可能尝试 Codex 兜底。旧配置和 Key 保存在 `~/Library/Application Support/EasyTranslate/config.json`，文件权限限制为当前用户。

## 使用方式

1. 默认 **⌥D** 打开或关闭翻译窗口；窗口固定在屏幕右上角。
2. 输入中文后约 90 ms 的短暂停顿就会开始翻译。可切换原文语言和目标语言，点击 **⇄** 可交换方向。
3. 点击“复制译文”复制全文并关闭窗口；译文框可选中部分文字后用 **⌘C** 复制。清空、重试和截图按钮都在窗口底部。
4. **⌥A** 读取当前选区并翻译；**⌘⌥S** 框选截图，使用本地 OCR 后翻译。划词需要辅助功能权限，截图需要录屏权限。选区翻译只展示和复制译文，不会改写原应用内容。

输入翻译、划词翻译和截图翻译的快捷键都能在“设置 → 录制”中自定义，保存后立即生效。支持 Command、Option、Control、Shift 与 A–Z、0–9、Space、F1–F12；至少需要一个 Command、Option 或 Control 修饰键。菜单栏图标显示“译”，应用不显示 Dock 图标。

## 术语和语气

本地 Marian 是专用机器翻译模型，不是可接受提示词的聊天模型，因此不能保证任意“客服口吻”重写。在线模型支持翻译提示词，但不能保证无误。内置 IT 术语表包含“固件、驱动程序、恢复出厂设置、故障排查、保修期”等中英双向词条。涉及付款、否定、时间、责任归属或专有名词时，请核对译文后再发送。

## 构建、测试与打包

要求 macOS 26+、Rust、Apple Command Line Tools。首次构建会下载 Rust 依赖；这与翻译模型缓存分开。

```sh
cargo test --locked
cargo fmt --check
cargo clippy --all-targets -- -D warnings
bash scripts/test-background.sh
./run.command
open dist/EasyTranslate.app
```

`scripts/test-background.sh` 不激活窗口、不模拟键鼠、不读取聊天或系统通用剪贴板，也不会下载翻译模型。打包脚本生成本机架构的 `dist/EasyTranslate.app`、ZIP 和 DMG，并使用本机固定签名；应用尚未公证，Intel 架构尚未实测。

## 已知限制

自动化测试覆盖取消、过期结果、缓存、快捷键、Ollama API 路径、菜单栏状态、OCR 错误和窗口布局；Google 在线质量尚未完成基准验收，也尚未对微信或 WhatsApp 做前台验收。当前 Marian 模型公开样例仍出现“周五前”译为 “on Friday”等语义偏差，不能宣称准确无误。正常启动时应用会在后台预热默认目标语言方向的 Marian 模型；缓存后可离线运行，首次准备模型需要网络。

诊断命令：

```sh
dist/EasyTranslate.app/Contents/MacOS/easytranslate --diagnostics
```
