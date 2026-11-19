# Windows 语音识别与翻译窗口：开发说明（2026-11-18 定稿）

本文是这一轮 Windows 端开发的完整说明，用户已确认方案。动手前先完整读根目录 `CLAUDE.md` 与 `docs/contributing.md`，并遵守其中的约定（中文交流、7 种界面语言同步、文档同步、clippy 干净）。

**范围边界**：只改 Windows 端（以及为它服务的共享 crate）；**不升版本号、不打包、不发布、不提交 git、不推送**。安卓不受影响，改共享代码时确认安卓/Linux/macOS 仍能编译（至少 `cargo check` 能过的部分）。

## 0. 现状（已读过代码，结论如下）

2026-11-17 已有一版 Windows 语音识别（见 `.workbuddy-ai/memory/2026-11-17.md`）：

- `apps/windows/server/src/audio.rs`：WASAPI 录音 16 kHz 单声道。
- `apps/windows/server/src/hotkey.rs`：WH_KEYBOARD_LL 低级键盘钩子，长按 Win 或自定义组合键触发。
- `apps/windows/server/src/dispatch/voice/`：语音状态机，`Router` 持有识别方（离线 SenseVoice / 豆包 / 无）。
- `apps/windows/server/src/ui/voice.rs`：屏幕下方的语音面板（分层窗口，GDI 自绘）。
- `apps/settings` 有「语音」页（`Feature::Voice` 仅 Windows），`[voice]` 配置在 `crates/qingjian-platform/src/config/voice.rs`。

翻译：青简上游有 `Ctrl+Alt+T` 翻译选中文字（`dispatch/translate`）与 `Ctrl+Alt+Y` 符号面板翻译组，**都保留不动**。翻译后端：`qingjian-predict` 的有道（`youdao.rs`）与 OpenAI 兼容接口（`CloudPredictor`，`PredictionKind::Translate`）。

### 必须修的现有问题

1. **识别结果不上屏，要再按一个键才出来**（最严重）。`advance_voice` 用 `deliver_clicked` 把结果放进 `Router.clicked`，但 DLL 只有在组句 / 翻译评审时才 `Poll`（见 `apps/windows/tsf/src/com/poll/mod.rs` 的 `poll_once`），平时只每 320 ms 发一次 `SyncMode`。修法见 §2.4。
2. 长按 Win 触发后**吞掉 Win 的 keyup**，系统的按键状态里 Win 可能一直是按下，下一个键变成 Win 组合键。改为「补发遮罩键、不吞抬起」（§2.1）。
3. `TranscriptEvent::Error` 时 `sink.error()` 后马上 `sink.hide()`，提示一闪就没。错误要停留约 2.5 秒（§2.3）。
4. 注释写 Esc 取消，实际发的是 `VoiceKey { down: false }`（结束并上屏）。改成真正取消（§2.1）。
5. 改 `[voice]`（服务或快捷键）后不生效，要重启 Server：`apply_config` 没重建识别方，钩子只在启动时装一次。改成热加载（§2.5）。
6. 小问题：`ui/voice.rs` 的 `utf16()` 把结尾的 NUL 也交给 `TextOutW`，会画出一个方框；`main.rs` 里 `#[cfg(not(windows))] fn serve(_router: Router)` 的参数与调用对不上。

## 1. 用户确认的效果

### 1.1 语音识别

- **缺省快捷键：按住右 Alt 说话，松开结束，文字写到光标处。** 按住不到 `hold_ms`（缺省 400 ms，可调）就松开，当普通按键放行。
- 可选：右 Alt / 右 Ctrl / 长按 Win / 自定义组合键 / 不用快捷键。设置里改完立即生效。
- 录音中按 Esc：取消这一段，不上屏（并吞掉这次 Esc）。
- 没开识别服务时触发：语音面板提示「语音识别未开启：请在 设置 → 语音 里选择识别服务」，停留约 2.5 秒。
- 语音面板底部加一行提示：按住触发时「松开结束 · Esc 取消」，点悬浮条触发时「再点「说」结束 · Esc 取消」。

### 1.2 悬浮状态条

由四格改为六格：`[中][，。][说][译][符][⚙]`。

- 「说」：点一下开始听，再点一下结束上屏；录音中这一格用强调色（`emphasized = true`）。
- 「译」：打开翻译窗口（窗口已在前台时收起）。
- 用文字格而不是新图标：节点树主题（`status.root`）按 `cells` 展开，文字格在所有主题下都能显示。

### 1.3 翻译窗口（新程序 `qingjian-translator.exe`）

- 缺省快捷键 **Ctrl+Alt+F**（全局，Server 的低级钩子拦截），可在设置的「快捷键」页改或关掉。悬浮条「译」也能打开。再按一次快捷键：窗口在前台就收起，否则带到前台。
- 布局（系统原生控件，跟随 DPI 缩放，字体 Microsoft YaHei UI）：

```
┌ 猛人翻译 ─────────────────────────────── ✕ ┐
│ 互译语言 [英语 ▾]            [⇄ 对调]        │
│ ┌──────────────────────────────────────┐  │
│ │ 原文输入框（多行，可用猛人打中文）     │  │
│ └──────────────────────────────────────┘  │
│ [🔊 朗读原文] [清空]                        │
│ ┌──────────────────────────────────────┐  │
│ │ 译文（只读，多行）                    │  │
│ └──────────────────────────────────────┘  │
│ [🔊 朗读译文] [复制译文]    状态文字        │
└────────────────────────────────────────────┘
```

- **自动互译**：原文主要是汉字 → 译成「互译语言」；否则 → 译成中文。互译语言可选：英语 en、日语 ja、韩语 ko、法语 fr、德语 de、西班牙语 es、俄语 ru、越南语 vi；缺省取 `[general]` 的学习语言，学习语言关着或是中文时用英语；选过的记住（写回配置，键名见 §3）。方向判断可参考 `qingjian_core` 里 `translation_target` 的统计方法（汉字 vs 假名 / 拉丁字母）。
- 停止输入约 0.8 秒、原文非空时自动翻译；内容没变不重发；`Ctrl+Enter` 立即翻译；`Esc` 收起窗口。
- 「对调」：把译文放进原文框并立即翻译（方向随之反过来）。
- 「复制译文」：全选译文框后 `WM_COPY`（不用直接调剪贴板 API）。
- 状态文字：「翻译中…」「有道翻译 · 完成」「AI 翻译 · 完成」、错误原因；两种服务都没开时提示「未开启翻译服务：请在 设置 → 云服务 开启有道翻译或 AI 服务」。
- **朗读**：Windows 自带 SAPI（`ISpVoice`，windows-rs 特性 `Win32_Media_Speech`），异步朗读，再点同一按钮或开始新的朗读时打断上一次（`SPF_PURGEBEFORESPEAK`）。按语言选声音：用 SAPI XML `<voice required="Language=409">…</voice>` 加 `SPF_IS_XML`（中文 804、英语 409、日语 411、韩语 412、法语 40C、德语 407、西班牙语 C0A、俄语 419、越南语 42A），文本要先做 XML 转义；没有对应声音时退回默认声音朗读（必要时去掉 XML 再念一次）。标志位数值：`SPF_ASYNC = 1`、`SPF_PURGEBEFORESPEAK = 2`、`SPF_IS_XML = 8`。
- 单实例：命名互斥体 + `FindWindowW` 找已有窗口；第二个实例负责把它带到前台（或收起）后退出。窗口关闭只隐藏，隐藏 10 分钟无操作再退出进程。
- 窗口位置：首次在主屏工作区居中偏上；拖动后记住（写回配置或内存均可，自行取舍，写配置就用 §3 的键）。

## 2. 实现方案

### 2.1 钩子 `apps/windows/server/src/hotkey.rs` 重写

- 状态里同时放「语音触发」与「翻译窗口组合键」两套；提供 `install(work, voice: VoiceTrigger, translator: Option<Combo>)` 与热更新用的 `reconfigure(...)`（只换 `STATE` 里的配置，不重装钩子）。
- `VoiceTrigger`：`HoldKey { vk, hold_ms }`（右 Alt = `VK_RMENU` 0xA5、右 Ctrl = `VK_RCONTROL` 0xA3、Win = 左右 Win 任一）、`Combo { mods, vk }`、`Off`。
- 低级钩子里分得出左右键（`KBDLLHOOKSTRUCT.vkCode`）。**忽略注入的事件**（`flags & LLKHF_INJECTED(0x10)`），自动重复的 keydown 不当新按下。
- 按住类：按下记时刻；按住期间按了别的**非修饰键**就作废这次（不触发，保证 Alt+Tab 这类组合不受影响）；计时线程 `tick_hold` 到点触发 `Work::VoiceKey { down: true }`，**先放开 `STATE` 的锁，再 `SendInput` 一次遮罩键**（VK 0xE8 按下 + 抬起），这样松开 Alt 不会激活菜单栏、松开 Win 不弹开始菜单、松开 Ctrl 不会被当成单击切中英；抬起时发 `VoiceKey { down: false }`，**不吞抬起**。
- 组合键：修饰键掩码支持 Ctrl / Shift / Alt / Win；主键用 `qingjian_platform::global_hotkey::binding()` 按当前键盘布局算虚拟键（支持符号键）。命中时吞掉主键的按下、重复与抬起。语音组合：按下开始、松开结束；翻译组合：按下发 `Work::OpenTranslator`。
- Esc：语音进行中（全局 `AtomicBool LISTENING`，由 Router 经回调置位）按 Esc → 吞掉 Esc 的按下与抬起，发 `Work::VoiceCancel`。
- 现有 `voice_trigger_flow` 测试按新状态机改写并补充：短按放行、按住到点触发、按住中按别的键作废、注入事件被忽略、翻译组合键、Esc 取消。全局状态的测试保持串行（放在一个测试里）。

### 2.2 `ipc::Work` 与工人循环

- `Work` 新增 `VoiceCancel`、`OpenTranslator`；`ipc/pipe.rs` 分派到 `router.cancel_hotkey_voice()`、`router.open_translator()`。
- `StatusSink` 新增 `fn open_translator(&self) {}`（缺省空实现），`UiHandle` 实现为起同目录的 `qingjian-translator.exe`（沿用 `launch_sibling`，它会先 `AllowSetForegroundWindow(ASFW_ANY)`）。

### 2.3 语音状态机 `dispatch/voice/`

- 目标会话：`self.mode_session.or(self.focused)`（`mode_session` 是最近来取模式的前台会话，更接近「当前光标所在应用」）。
- 新增 `toggle_status_voice()`：没在听就开始；在听且还没松手就结束；正在等最终结果时忽略。
- 入口不可用时 `sink.error("语音识别未开启：…")`，并进入「提示停留」状态：`VoiceRuntime` 记 `notice_until: Option<Instant>`，到点 `sink.hide()`；`voice_active()` 在提示停留期间也返回真，保证 tick 节拍够短。识别出错（`TranscriptEvent::Error`）同样停留后再收。
- 开始 / 结束 / 取消时：调监听回调（让钩子知道 Esc 要不要拦）并 `reconcile_status()`（让「说」格变色）。`StatusView` 加 `voice_active: bool`。
- `VoiceSink` 的 `listening` / `recognizing` 带上提示行内容（按住触发还是点按触发），面板据此画底部提示。

### 2.4 结果上屏（协议小改 + DLL）

- `ServerMessage::ModeSync` 加字段 `#[serde(default)] commit: Option<String>`。
- `ClientMessage::SyncMode` 加字段 `#[serde(default)] accepts_commit: bool`：新 DLL 发 `true`；老 DLL 不带（反序列化为 `false`）。Server 只在 `accepts_commit` 为真时把 `take_clicked(session)` 放进 `ModeSync.commit`，否则留着（老 DLL 下次按键 / 轮询时仍能取走），**不丢字**。
- **不升 `PROTOCOL_VERSION`**（Linux 插件写死了这个数，本轮不动 Linux）；靠 serde 缺省字段与上面的能力标志兼容。在协议文档注释里写清楚：新 Server + 老 DLL 时语音结果要等下一次按键才上屏，重开应用（换新 DLL）后正常。
- DLL：`client/engine.rs` 的 `sync_mode` 发 `accepts_commit: true`，把 `commit` 带进 `ModeSyncReply`；`com/poll/mod.rs` 的 `sync_mode()` 拿到 `commit` 后，像 `commit_clicked` 那样用 `last_context()` + `request_update(..., Some(commit), String::new())` 写进文档；没有上下文就记日志丢弃。
- 同步更新所有构造 `ModeSync` / `SyncMode` 的地方：`crates/qingjian-platform/tests/protocol/variants.rs`、`apps/windows/server/tests/engine_loop/status.rs`、`apps/windows/server/src/dispatch/message.rs`、`apps/windows/tsf/src/client/engine.rs`，以及 grep 出来的其他地方。
- 加一条 Router 测试：语音 Final 之后，`SyncMode { accepts_commit: true }` 的回包带出文字；`accepts_commit: false` 时不带、之后 `Poll` 仍能取到。

### 2.5 配置热加载

- `dispatch/reload/mod.rs` 的 `apply_config`：在覆盖 `self.config` 之前比较 `[voice]`；识别相关字段变了就 `configure_voice`；触发相关（`hotkey` / `hold_ms` / `shortcut`）或 `[shortcut] translator` 变了就调 Router 上注入的 `hotkey_reconfigure` 回调（`main.rs` 注入，调 `hotkey::reconfigure`）。Router 保持平台无关：回调是 `Box<dyn Fn(...) + Send>`。

### 2.6 悬浮状态条

- `ui/status/mod.rs`：`ACTIONS` 改成六项，`status_cells()` 与 GDI 的 `cells()` 同步加「说」「译」；`placement/action.rs` 加 `ToggleVoice`、`OpenTranslator`；`placement/mod.rs` 的 `on_click`：`ToggleVoice` → `StatusEvent::ToggleVoice` 回 Router，`OpenTranslator` → UI 线程直接起程序（同齿轮打开设置）。
- `dispatch/status/event.rs` 加 `ToggleVoice`；`handle_status_event` 处理。
- 检查 `qingjian-render` 的状态条测试是否对格数有假设（`cells_have_increasing_edges...` 只用三格，不受影响）。

### 2.7 语音面板 `ui/voice.rs`

- 修 NUL；画底部提示行；`VoiceView` 加 `hint` 字段。

### 2.8 新程序 `apps/windows/translator`

- 新 crate `qingjian-windows-translator`，bin 名 `qingjian-translator`，版本与 Server 相同（0.4.8，不升）。加入 workspace `members`；`Cargo.lock` 让 cargo 生成（**用户的打包脚本带 `--locked`，所以要先跑一次不带 `--locked` 的 `cargo build -p qingjian-windows-translator` 把锁文件补上**）。
- 依赖：`qingjian-core`、`qingjian-platform`、`qingjian-predict`、`tracing`、`tracing-subscriber`、`tracing-appender`、`dotenvy`（读 `%APPDATA%\Qingjian\.env`）、`windows 0.62.2`（`Win32_Foundation`、`Win32_Graphics_Gdi`、`Win32_System_Com`、`Win32_System_LibraryLoader`、`Win32_System_Threading`、`Win32_UI_HiDpi`、`Win32_UI_Input_KeyboardAndMouse`、`Win32_UI_WindowsAndMessaging`、`Win32_Media_Speech`）；构建依赖 `embed-manifest`（要 Common Controls v6 与 PerMonitorV2，按钮才是现代样式）、`winresource`（嵌 `apps/windows/tsf/resources/qingjian.ico`）。参考 `apps/windows/server/build.rs`。
- 非 Windows 目标：`main` 只打印一句「仅 Windows」，保证 workspace 在其他平台能编。
- 翻译调用：启动时 `qingjian_platform::member::install()`（装有道密钥读取器）、读配置。有道开着 → 直接 `qingjian_predict::youdao::client()` + `youdao::translate()`（在后台线程的 tokio 运行时里跑；没有 tokio 依赖就用 `CloudPredictor`）；否则 AI 服务开着 → `CloudPredictor::new(&cfg)`，`cfg` 是 `[predict]` 的拷贝并把 `debounce_ms`、`min_interval_ms` 设为 0，提交 `PredictionRequest { kind: PredictionKind::Translate, manual: true, text, target_language, .. }`，用 `WM_TIMER` 轮询 `poll()` 取 `sentence`。按序号丢弃过期结果。`CloudPredictor` 走的 `ChatClient::complete` 本身也会在有道开启时走有道，二选一实现即可，以简单可靠为准。
- 日志写 `%LOCALAPPDATA%\Qingjian\logs`（前缀 `translator`），密钥走 `logs::secrets::MaskingWriter`。

### 2.9 配置 `crates/qingjian-platform`

- `VoiceConfig` 加 `hotkey: String`：`right_alt` / `right_ctrl` / `win` / `shortcut` / `none`；缺省空串 = 自动：`shortcut` 有值时按组合键，否则右 Alt。加方法返回枚举（平台无关的 `VoiceHotkey`）。模板里写成注释行 `# hotkey = "right_alt"` 并说明各取值（模板必须解析成默认配置，`template_parses_to_defaults` 测试会核对）；`hold_ms` 注释改成「按住右 Alt / 右 Ctrl / Win 多久开始」；示例组合键别用 `ctrl+alt+v`（与剪贴板冲突），改成 `ctrl+alt+s`。
- `ShortcutConfig` 加 `translator: Option<KeyCombo>`（`optional_key_combo`，空串关闭），缺省 `Some(KeyCombo::TRANSLATOR_DEFAULT)` = `control+option+f`。Windows 模板 `[shortcut]` 写 `translator = "ctrl+alt+f"`。
- 翻译窗口记忆的互译语言：放在一个新分节 `[translator]`（`language`，缺省空 = 跟学习语言），或直接写进 `%APPDATA%\Qingjian` 下的小文件；任选其一，写清楚。
- 补 serde 往返与旧配置兼容的测试。

### 2.10 设置程序 `apps/settings`

- 「语音」页「触发方式」：新增「快捷键」选择（右 Alt / 右 Ctrl / 长按 Win / 自定义组合键 / 不用快捷键），选「自定义组合键」时下面出现录键行（`Shortcut::Voice`，写 `[voice] shortcut`，同时把 `hotkey` 写成 `shortcut`）；时长行在「不用快捷键 / 自定义组合键」时隐藏或禁用。文案里的「按住 Win 键说话」都改成按当前选择描述。
- 「快捷键」页加「打开翻译窗口」录键行（`Shortcut::Translator`，写 `[shortcut] translator`），用 `Feature::Voice.here()` 限定 Windows（或新增 Feature，自行判断，但要同步 Feature 的各张表与测试）；「恢复默认」一并恢复它。
- `apps/settings/src/shortcut.rs`：两个新变体的标题、配置键、提示、当前值、`accept` 规则（至少两个修饰键 + 字母 / 数字 / 符号；与翻译选中文字、AI 联想、翻译组、剪贴板、私密输入等已有组合键查重；两者互相查重）。`ALL` 数组与测试同步。
- 7 个 `locales/*.toml` 全部补齐新文案（zh-Hans、zh-Hant、en、ja、es、vi、de），`bamboo-slip-i18n` 的一致性测试要过。

### 2.11 安装包

- `apps/windows/installer/build.ps1`：`cargo build` 加 `-p qingjian-windows-translator`；产物检查列表加 `release\qingjian-translator.exe`（`-Sign` 时一并签名）。
- `apps/windows/installer/qingjian.iss`：`[Files]` 装 `qingjian-translator.exe`；安装前 / 卸载时与其他 exe 一样 `taskkill`（`[Code]` 的 `KillProcess` 与 `[UninstallRun]`）；开始菜单可加「猛人翻译」快捷方式。

### 2.12 文档

- `docs/design/voice-and-swipe.md` 补 Windows 触发方式、遮罩键、结果上屏路径与协议兼容说明。
- `docs/notes/crate-notes.md` 补新 crate 与钩子要点；`CLAUDE.md` 目录地图在 `apps/windows` 一行加 `translator`。
- 新增用户文档 `docs/user/platforms/windows-voice-translate.md`（面向用户、不出现实现词）：怎么开语音、快捷键、悬浮条两格、翻译窗口、朗读需要系统语音包。
- `docs/design/mengren-public.md` 删掉「新增语音入口和悬浮窗改造暂缓」的说法，改成本轮已开发、待真机验收；`docs/plan/todo.md` 记待验收项。
- 不改 `CHANGELOG.md` 与版本号（发版时再写）。

## 3. 验收（编译环境内能做的）

在仓库根目录依次跑，全部通过才算完成；失败就修，修完从头再跑：

```
cargo fmt --all
cargo build -p qingjian-windows-translator          # 不带 --locked，先补 Cargo.lock
cargo build --locked -p qingjian-windows-server -p qingjian-windows-tsf -p qingjian-settings -p qingjian-windows-translator
cargo test -p qingjian-platform
cargo test -p qingjian-windows-server
cargo test -p qingjian-windows-tsf
cargo test -p qingjian-settings
cargo test -p qingjian-render
cargo clippy -p qingjian-platform -p qingjian-windows-server -p qingjian-windows-tsf -p qingjian-settings -p qingjian-windows-translator --all-targets -- -D warnings
```

已知坑（见 `.workbuddy-ai/memory/MEMORY.md`）：本机 build script 调 `reg.exe` 可能被安全策略拦截，cargo 退出码非 0，但以 `test result:` 行与真正的编译错误为准，不要反复重试 `reg.exe`。

最后在仓库里写一份结果记录 `docs/plan/windows-voice-translate-result.md`：改了哪些文件、各项检查结果、哪些只能真机验收（麦克风、TSF 上屏、朗读声音、热键在各应用里的表现）。**不打包、不安装、不发布**；打包验收由用户决定。
