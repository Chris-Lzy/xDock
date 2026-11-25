# xDock

基于 Rust + Direct3D 11 + Direct2D + DirectComposition 打造的轻量级高性能 Windows 桌面 Dock 与全屏启动台（支持 Windows 10 / Windows 11）。

## 核心特性

* **纯原生 GPU 硬件合成**：使用 `D3D11 + Direct2D + DirectComposition`（Premultiplied Alpha），事件驱动按需渲染，待机 0% CPU 占用。
* **macOS 风格 Dock 动效**：高斯波浪放大、底边基准向上生长、相邻图标推开与点击回弹，兼容 60Hz / 120Hz+ 显示器。
* **全屏三栏工作台 (`Alt + Space`)**：
  * **正在运行栏**：实时展示前台与托盘主应用，支持滚轮滚动、点击切换/最小化、关闭前二次确认。
  * **快速直达栏**：支持可视化编辑固定与移除，配置自动持久化至 `%APPDATA%\xDock\config.ini`。
  * **应用网格与搜索**：智能过滤附属工具，支持分类切换、实时搜索、未读消息角标提示。
* **替代系统任务栏**：
  * 启动时自动隐藏系统任务栏，内置 `--watchdog` 内核级守护进程确保正常退出或异常强杀均能自动恢复任务栏。
  * 伴生「系统状态岛」集成网络、音量（点击静音/滚轮调节）、输入法（中/英）、时间日期、设置、电源菜单（锁屏/睡眠/重启/关机）。
* **高度可定制**：支持自定义 Dock 高度（`30 ~ 120 px`）、放大倍数、网格列数、玻璃透明度、右键隐藏图标与开机自启动（关闭无残留）。

## 编译构建

```powershell
cargo build --release
```

生成的可执行文件位于 `target\release\xDock.exe`。
