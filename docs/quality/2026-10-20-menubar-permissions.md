# 菜单栏与授权修复验证

## 最终观测

2026-10-20 15:20，将最终签名包安装并注册到 /Applications/EasyTranslate.app 后，经 Finder/LaunchServices 启动，顶部截图明确显示“译”（铃铛与电池之间）。Thaw 已恢复运行；没有更改它的隐藏规则。菜单栏系统跟踪记录已识别 com.sorigedo.easytranslate 且 isAllowed=true。之前开发目录启动时最小 Swift 状态栏探针也未显示，因此不能把问题单独归因于文字渲染、手动事件循环或 Thaw。

应用现在为 accessory / LSUIElement 模式，无 Dock 项，启动不弹窗口。保留稳定状态项名称与透明“译”字模板图标，采用 AppKit 原生事件循环、定时器和本地按键监听。菜单回归脚本验证设置、翻译、权限、退出入口；屏幕可见性通过顶部截图另行确认。

## 权限根因及未完成项

tccd 日志明确记录 Failed to match existing code requirement，旧授权 cdhash 与新版不同。临时签名会随构建变化；系统设置开关开启不代表当前签名已授权。正在运行的 /Applications 版本菜单显示“未授权当前版本”，须用户移除辅助功能旧条目，重新添加 /Applications/EasyTranslate.app。未自动授予或改写权限数据库。授权后菜单每秒更新状态，需重新在输入框按快捷键。真实聊天读写未测试。

所有 Rust 测试、fmt、clippy、Swift URLSession 故障模拟、语言检测、包内桥接检测及签名校验通过。实际快捷键录制重复回归仍待前台空闲时完成，不能以编译通过代替。

探索性调整仅对 EasyTranslate 自身 preferred position 设置为 180，未改变其他应用位置设置；最小探针已退出。已有 AGENTS.md 未改动。
