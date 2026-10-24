# EasyTranslate 首版设计与开源方案调研

日期：2026-10-19。状态：待用户确认的设计，不代表功能已经实现。

## 已确认需求

- 项目地址：https://github.com/Sorigedo/EasyTranslate.git 。GitHub API 返回 size=0，内容接口返回 404；当前未发现可复用的项目代码。
- 面向 macOS，使用全局快捷键从任意应用呼出独立翻译窗口，不绑定某个聊天软件。
- 输入中文时自动翻译，默认英文，可以选择其他目标语言；确认后将译文填回调用前的输入框，不自动发送消息。
- 支持快捷键划词翻译、句子/文章翻译与截图翻译。
- 优先适配微信与 WhatsApp；全局唤起不绑定特定软件，但回填兼容性逐项实测。
- 后续继续输入时自动更新译文。当前建议指翻译窗口中的连续编辑；是否还需直接监听聊天软件原生输入框，仍待确认。
- 参考 Easydict，优先使用开源、免费或可本地运行的翻译能力，避免把可用性押在一个不稳定接口上。

## 已核实的参考方案

### Easydict

官方仓库未归档，许可证标识 GPL-3.0。开发分支使用指南列出 Option+A 输入翻译、输入时自动查询、划词查询、截图翻译和可配置的 translate-and-replace 快捷键。

其 AppleTranslation / TranslationView 源码通过 macOS 15+ 的 Translation 框架工作。官方指南说明需启用离线翻译、下载对应语言包；失败时不会自动转用快捷指令。Bing 实现包含 token 无效时的重试处理。这证明其服务有不同实现路径，不能由用户遇到一次失败就判断所有接口均失效。

用户当前安装版本、失败服务和错误信息未知，因此本轮没有定位 Easydict 的具体故障，也没有测试其在线接口可用性。参考交互与公开平台能力，自行实现本项目；如后续直接复用源码，先按所选项目许可证处理归属与分发要求。

### Apple Translation + Vision：推荐首版默认

Apple 文档确认 TranslationSession 从 macOS 15 引入；Vision 的 VNRecognizeTextRequest 提供图片文字识别。Easydict 的文档与代码也验证了本地翻译集成路径。

这是系统框架方案，不是开源翻译引擎；不需要用户注册第三方翻译 API。首次使用须检测语言支持并提示下载模型，不能承诺任意语言都支持。OCR 识别在本地完成，识别文字送往当前选中的翻译引擎。需实际验证语言包下载、翻译质量和当前系统兼容性。

### LibreTranslate + Argos Translate：开源后端选项

LibreTranslate 是 AGPL-3.0 自部署翻译 API，基于 Argos Translate；Argos 仓库标识 MIT，提供 Python 离线翻译库。开源软件免费不等于公共服务器免费、无限额或始终可用。推荐对接用户自己的本地或自部署地址，不内置未经验证的公共节点。

当前实现提供用户配置的 LibreTranslate 兼容服务：用户在设置中填写实例根地址和可选 API Key，客户端调用 `/translate`，并遵循联网授权后才发送原文。旧配置载入时会补入该服务且保留原有服务选择。客户端不安装 Python、不下载 Argos 模型，也不部署服务器。公共服务器可能要求 API Key、限流或不可用；模型与语言对可用性、模型许可证、中文聊天效果及本地资源消耗需由用户按具体部署方案核验。Google 网页接口仍作为独立选项保留，其上游限流/变更不由客户端控制。

## 首版实现建议（由开发者提出，待确认）

使用 SwiftUI + AppKit，最低 macOS 15；提供菜单栏入口及可配置全局快捷键。暂定 Option+Space 为输入窗口快捷键，若注册冲突必须提示用户修改。选择文本翻译、截图翻译分别有独立菜单项与可配置快捷键。

### 输入与回填

1. 呼出前记录前台应用及可访问的输入控件，不自动读取全部聊天记录。
2. 打开浮窗并聚焦中文输入框；中文输入法组词期间不请求翻译，提交文字后停顿约 500 毫秒触发。
3. 取消旧请求，按输入版本校验结果，过期译文不覆盖新结果。显示原文、可编辑译文、目标语言及明确的下载/翻译/失败状态。
4. 当前输入尚未得到对应译文时禁用确认。确认按钮或 Command+Return 插入当前编辑后的译文；普通 Return 保留用于多行输入，Esc 取消。
5. 恢复原应用与原控件，验证焦点和目标仍有效后插入。优先评估辅助功能 API，必要时使用剪贴板粘贴。禁止无条件全选覆盖现有草稿；保留光标/选区语义。
6. 目标不存在、原窗口已关闭、焦点无法可靠恢复或粘贴失败时，保留结果并显示复制按钮，不向不确定的窗口模拟输入。
7. 使用临时剪贴板时保存可保存的数据类型，仅在剪贴板仍属于本次操作时恢复，避免覆盖用户新复制的内容；不得声称所有应用和数据类型均能无损恢复。
8. 从菜单栏独立打开且没有有效输入目标时照常翻译，提供复制；不会强制回填。

### 划词翻译

首版先实现选中文字后按快捷键，支持单词、句子和可获取的多段文本。鼠标划词后浮现按钮作为后续增强。只在用户主动触发后查询，不在每次选中文字时自动发送内容。

通过 Accessibility 获取选中内容和位置；应用未暴露选区时明确提示不可读取，提供手动复制后翻译入口。首版不使用自动 Command+C 兜底，以免破坏剪贴板或误触应用快捷键。划词结果默认只展示和复制；替换选区须另点明确的替换按钮并验证原目标。

### 截图翻译

用户触发后进入框选模式，按 Esc 取消；使用 ScreenCaptureKit 获取授权区域图像，Vision 识别文字，按阅读顺序展示原文与译文。首版不做贴图覆盖或复杂排版还原；不默认保存截图，不自动把截图内容粘回聊天框。截图翻译默认自动检测原文语言，翻译到中文；输入翻译默认中文到英文，两者分别保存目标语言。

### 服务、权限与数据

统一 TranslationProvider 接口负责语言支持、异步翻译、取消和可读错误；UI 不绑定某家服务。LibreTranslate 是用户主动配置的可选后端。禁止在离线后端失败后静默上传到云端。

辅助功能权限用于选区读取及回填；录屏权限用于截图，只在相应功能首次使用时解释并请求。用户拒绝相关权限时，手动输入翻译仍可用。未来密钥存入 Keychain，不记录正文、截图或密钥；默认不保留历史。

## 验收与交付范围

- 构建可运行的 .app，并提供可复现构建/启动命令；不要求本轮购买开发者账号或发布应用商店。
- 测试文本输入、中文组词、连打时过期结果、手动编辑译文、取消、语言包缺失及服务错误。
- TextEdit 和浏览器文本框验证回填、选区、已有草稿、窗口关闭与焦点变化；聊天软件逐项记录兼容性，不能以全局快捷键可用推断所有输入框均可回填。
- 测试辅助功能/录屏权限拒绝、多显示器截图、取消截图和 OCR 无文字。
- 自动测试覆盖取消及过期结果控制、配置解析与可测试的服务逻辑；系统权限与跨应用行为使用人工验收。
- 未完成上述验证前，不声称接口稳定、兼容所有聊天软件或翻译质量已达标。

## 连续输入与文章翻译

聊天模式下，在浮窗继续输入或修改中文时自动更新译文。回填后收起浮窗、恢复聊天框焦点；已经填入的草稿不被后台反复覆盖。再次调用时重新记录目标，保留上一份翻译草稿并提供“新建”；不默认重复追加上次译文。

如果用户要求直接在微信/WhatsApp 自己的输入框打中文时持续显示译文，需要额外实现输入控件监听、输入法处理与客户端适配。这不属于普通全局快捷键能力，应先验证客户端是否暴露相应辅助功能事件。

阅读模式支持选择或粘贴文章，按段显示原文与译文，提供单段/全文复制。文章使用“翻译全文”按钮，避免每次按键重译全文；按后端长度限制分段，保持顺序与空行，显示进度，支持取消及失败段落重试。未完成时不得显示全文成功。首版不抓网页、不导入 PDF、不重建文档排版。

## 其他成熟产品与可复用组件

以下结论来自 2026-10-19 读取的项目官方 README 与 GitHub API，不代表实机体验或接口成功率测试。

| 项目 | 已核实能力 | 参考或复用方式 |
| --- | --- | --- |
| Easydict | macOS 输入、划词、截图、自动查询，多后端；GPL-3.0 | 核心交互与原生系统集成参考 |
| Bob | macOS 划词、截图、输入翻译，OCR 和插件 | 参考三个快捷键入口；当前仓库元数据没有许可证，不能据此当作可自由复用的完整开源产品 |
| NextAI Translator | 原 openai-translator 地址已重定向到该项目；跨平台、划词、截图、流式输出；AGPL-3.0 | 参考句子/文章交互；README 的 API 配置要求说明客户端开源不等于推理服务免费 |
| STranslate | WPF 翻译/OCR 工具，MIT | 参考流程，不直接迁移 Windows UI |
| KeyboardShortcuts | Swift 全局快捷键包，含配置录制控件，MIT | 可评估引入，减少热键管理代码 |

许可证标识来自仓库元数据；实际复用需检查锁定版本的 LICENSE。本轮没有复制这些项目代码。

## 快速交付顺序

建议采用轻量原生 SwiftUI + AppKit 客户端，复用系统框架和小型组件。直接 fork Easydict 可复用既有 OCR/划词/权限处理，但聊天草稿与回填仍需开发，也需维护它的完整项目结构；仅在明确接受该路线时再选用。

### 阶段 0：验证关键能力

先验证 Apple 中英翻译与模型下载，再验证微信/WhatsApp 的输入焦点和回填。用测试草稿，记录客户端版本、桌面/网页版与权限状态，不自动发送。验证完成后才能估计可靠工期。

### 阶段 1：聊天翻译闭环

菜单栏与全局唤起 → 中文组词处理 → 停顿自动翻译 → 可编辑译文 → 回填原输入框 → 再次继续使用。包含目标语言、复制兜底、旧结果隔离、焦点检查与错误提示。默认 Apple 翻译。

验收微信、WhatsApp 的原有草稿、光标、选区、取消、重复调用及窗口切换。不支持的控件明确退回复制操作。

### 阶段 2：快捷键划词、句子与文章

共用同一翻译后端，新增选区读取和阅读视图。短文本即时查询，长文章按段处理，提供进度、取消和失败重试。暂缓鼠标自动弹按钮、网页抓取与 PDF 导入。

### 阶段 3：截图翻译

框选截图 → 本地 OCR → 可修正原文 → 翻译 → 复制。复用阅读视图，测试多显示器、权限拒绝、无文字和取消。首版优先单列聊天/文章截图；多栏、表格阅读顺序提示检查，不承诺还原原图排版。

### 后续扩展

LibreTranslate 连接测试、其他用户自带密钥的官方后端、主动选择的失败切换、鼠标划词按钮、更复杂输入框兼容。离线失败不静默切换联网。

不自研 OCR/翻译模型，不依赖未经验证的公共免费节点或网页逆向接口。开源客户端、开源引擎、免费云端额度是三个不同问题，分别验证。

## 原始资料

以下资料在本轮通过 GitHub API / 官方文档读取；开发分支与已安装发行版可能不同。

- https://api.github.com/repos/Sorigedo/EasyTranslate
- https://github.com/tisfeng/Easydict
- https://github.com/tisfeng/Easydict/blob/dev/docs/user-docs/en/GUIDE.md
- https://github.com/tisfeng/Easydict/blob/dev/docs/user-docs/en/How-to-use-macOS-system-translation-in-Easydict.md
- https://github.com/tisfeng/Easydict/blob/dev/Easydict/Swift/Service/Apple/AppleTranslation/TranslationView.swift
- https://github.com/tisfeng/Easydict/blob/dev/Easydict/Swift/Service/Bing/BingService%2BTranslate.swift
- https://developer.apple.com/documentation/translation/translationsession
- https://developer.apple.com/documentation/vision/vnrecognizetextrequest
- https://github.com/LibreTranslate/LibreTranslate
- https://github.com/argosopentech/argos-translate
- https://github.com/ripperhe/Bob
- https://github.com/nextai-translator/nextai-translator
- https://github.com/STranslate/STranslate
- https://github.com/sindresorhus/KeyboardShortcuts


## Implemented design amendments (2026-10-19)

User-approved changes supersede the initial floating-input/SwiftUI design: Rust owns native AppKit windows and accessibility integration; a narrow Swift helper calls Apple Translation and Vision. A global hotkey binds the current editable chat field in any app exposing supported AX attributes. Input translation defaults to Option+A, selection to Option+D, screenshot to Command+Option+S, with an immediately applied settings window. Confirmation replaces the exact verified draft and never sends it. App and menu-bar identity use the character 译. A root run.command builds the app and ZIP.

Free local translation is implemented, but semantic accuracy is not guaranteed: the public payment example changes the payer. Direct WeChat/WhatsApp acceptance remains outstanding. See README.md and docs/quality/2026-10-19-local-benchmark.json for current evidence and limits.
