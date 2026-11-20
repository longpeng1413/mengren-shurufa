# Windows 语音、翻译与剪贴板复查结果（2026-11-20）

对照 [windows-voice-translate.md](windows-voice-translate.md) 与 Codex 的记录（[windows-voice-translate-result.md](windows-voice-translate-result.md)、`mengren/releases/v0.4.9～v0.4.11.md`）复查已发布的 Windows 0.4.9～0.4.11。本轮只改 Windows 端及其共享代码。复查完成后用户决定直接发布，Windows 升为 0.4.12（发布说明见 `mengren/releases/v0.4.12.md`），安卓保持 0.4.8；笔记本效果待用户更新后复测。

**0.4.12 发布后的事故与 0.4.13**：用户在台式机更新 0.4.12 后，按住右 Alt 时任务栏出现麦克风图标，但语音面板不出现、松开无文字。server 日志显示每次 `voice started` 后约 3 秒 Server 重新启动；Windows 事件日志记录 `qingjian-server.exe` 在 `USER32.dll` 内 0xc0000005（非法内存访问）。根因是本轮新加的面板换行绘制：开始听写时面板第一帧文字为空，空字符串转成的空切片指针是占位地址，交给 `DrawTextW` 后系统函数读取该地址崩溃。原单元测试只测了有字的情况，漏了空文字。修复：`ui/voice.rs` 的 `draw_text` 对空文字直接跳过；补测试 `panel_paints_empty_long_and_error_views` 在内存画布上完整绘制空文字、长结果、出错三种面板（修复前该测试同样以 0xc0000005 崩溃）。Windows 升为 0.4.13。

说明：工作区里 Codex 这三版的源码改动本地都还没有提交，`git diff` 分不出两轮各改了什么，下面「改了哪些文件」只列本轮的改动。

## 一、笔记本语音识别用不了（最优先）

用户尚未提供笔记本日志，以下结论来自读代码与本机实测。

### 根本原因

**录音初始化强要 16 kHz 单声道，而笔记本麦克风的混音格式不是这个。** 旧代码以共享模式调用 `IAudioClient::Initialize`，标志位为 0，直接要 16 kHz / 单声道 / 16 位。共享模式不带转换标志时，系统只接受和设备「混音格式」采样率、声道数一致的格式，否则返回 `AUDCLNT_E_UNSUPPORTED_FORMAT`（0x88890008）。

本机实测证实了这一点：台式机用的是「5MP USB Camera-Audio」摄像头麦克风，它的混音格式恰好是 **16000 Hz 单声道**，所以旧代码在台式机上能用。笔记本内置阵列麦克风通常是 48 kHz 双声道或四声道，旧代码在这一步就失败了。

失败之后用户什么也看不到，原因是第二个缺陷：录音线程出错只写一行「语音录音线程退出」日志，录音工厂起完线程就返回成功，面板一直显示「正在听」、没有波形，松手后识别方收不到任何音频，结果为空又被静默收起。

### 七个怀疑点逐条核实

| # | 怀疑点 | 结论 | 处理 |
| --- | --- | --- | --- |
| 1 | 共享模式格式不匹配 | **成立**，是主因（见上） | 先带 `AUTOCONVERTPCM \| SRC_DEFAULT_QUALITY` 请系统转换；被拒再换一个新客户端按 `GetMixFormat` 录，进程内折成单声道并重采样到 16 kHz，支持 int16 / int24 / int32 / float32 和扩展格式头 |
| 2 | 录音线程错误只进日志 | **成立** | 工厂等录音线程真正 `Start`（最多 1.5 秒）才返回；打不开时面板显示分类后的原因；录到一半出错经新的 `failure` 通道让 Router 结束这一段并提示 |
| 3 | 用默认通信设备 | **成立** | 改为先取默认录音设备（eConsole），取不到再退回默认通信设备 |
| 4 | 隐私设置 / 静音键没有提示 | **成立** | 拒绝访问（0x80070005）直接提示去「设置 → 隐私和安全性 → 麦克风」并检查静音键；整段原始峰值低于约 −50 dBFS 且没识别出字时，提示「没有收到声音」并给出同样的检查步骤 |
| 5 | 识别结果为空时静默收起 | **成立** | 有声音时提示「没有听清，请再说一次」，几乎没声音时提示检查静音键与权限 |
| 6 | 焦点会话要求 | **部分成立** | 会话编号两端都用线程号（DLL 用 `GetCurrentThreadId`，Server 用焦点窗口所属线程），Chrome、Edge、微信、记事本里只要当前输入法是猛人就能对上；但原提示没说要切到猛人输入法，已改为「请先点进输入框，并把输入法切到猛人输入法，再按住说话」。各应用实际表现需真机确认 |
| 7 | 离线模型 / 引擎加载失败只进日志 | **成立，且更糟** | 原来这种情况面板显示「语音识别未开启」，误导用户。现在分别提示「还没下载离线语音模型」「离线识别引擎加载失败，请重新安装」「豆包语音识别还没填 API Key」，都没选才说「未开启」。引擎库加载失败时，系统给的原因（缺依赖、位数不对等）也保留进日志 |

顺带排除的一个可能：随包的 `sherpa-onnx-c-api.dll`、`onnxruntime.dll` 只依赖 KERNEL32、ADVAPI32、dxgi 等系统自带库，不需要另装 VC++ 运行库，所以「笔记本缺运行库」不是原因。

### 新增的诊断日志（info 级，在 server 日志里）

拿到笔记本日志后，按这几行看卡在哪一步：

- `voice capture device`：设备名、用的是默认录音设备还是通信设备（`role`）、混音格式（如 `48000 Hz x 2 ch Float32`；认不出的格式会写原始字段）。
- `voice capture Initialize (system conversion)` / `(mix format, converted in process)`：每次初始化的标志位和 HRESULT。第一行 `hr=0x88890008`、第二行 `hr=0x00000000` 就是本次修复生效的路径。
- `voice capture started` / `voice capture stopped`：每段录音的时长、包数、静音包数、原始峰值和 dBFS。静音包数等于包数、或峰值接近 0，说明被隐私设置或静音键挡住。
- `voice started` / `voice final`：识别方类型（offline / volcengine）、结果字数、峰值、录音时长。
- `voice unavailable`、`voice trigger without a Mengren input session`、`voice recognition error`、`offline speech engine unavailable`：各类不可用原因。
- `voice final delivered to input thread` / `sent as Unicode input` / `dropped: focus changed` / `expired before delivery`：结果交付走了哪条路。
- `global shortcut configured` / `global shortcut hook installed`：快捷键配置与钩子安装。

## 二、Codex 改动复查

按严重程度排列。「已修」表示本轮直接修了；「待定」需要你拍板。

### 已修

1. **（严重）录音格式不匹配与静默失败**：见第一节。
2. **（严重）快捷键钩子装在 UI 线程上，可能被系统静默卸载。** 低级键盘钩子必须在 `LowLevelHooksTimeout`（约 1 秒）内返回，超时多次后 Windows 7 起会静默卸掉钩子，之后右 Alt 等快捷键全部失灵直到重启 Server。UI 线程还要画候选窗、状态条和语音面板，性能弱的笔记本更容易碰上。改为专用高优先级线程装钩子并跑消息循环。
3. **（中）误触发录音。** 原来按住触发键期间只有「非修饰键」会作废。现在又按下别的修饰键（如按住 Win 再按 Shift 准备 Win+Shift+S 截图）、点鼠标或滚滚轮（右 Ctrl + 点击多选、Ctrl + 滚轮缩放）也作废。鼠标用新增的低级鼠标钩子判断，只读不拦。
4. **（中）带 Win / Alt 的自定义组合键松开后弹开始菜单或激活菜单栏。** 组合键命中时主键被吞，系统以为只按了 Win / Alt。现在吞键后同样补发遮罩键（0xE8）。
5. **（中）录音中途断开没有处理**：见第一节第 2 条。
6. **（中）语音面板长提示被截断。** 面板文字原是单行 `TextOutW`，超出 420 像素宽就截掉，隐私设置那类提示只显示半句。改为区域内自动换行；出错时文字从波形位置排到底；中间识别结果放不下时保留最新的几行，前面加省略号。
7. **（中）翻译窗口在 125% / 150% 缩放的屏上首次打开尺寸不对。** 窗口按 660×615 物理像素创建，控件却按缩放比例摆放，150% 时按钮挤出窗口、互相重叠（最小尺寸只在拖动时生效）。现在创建后按所在屏的缩放放大到设计尺寸。
8. **（中）翻译失败后无法重试。** 原文没变时「翻译」按钮和 Ctrl+Enter 被当成「内容没变不重发」吞掉，断网恢复后只能改字才能重试。现在上次没拿到译文时允许立即重发。
9. **（低）剪贴板「全部清空」与单条删除不一致。** Codex 修了单条删除在对方设备时钟偏快时删除标记落后的问题（`MAX(now, updated_at + 1)`），清空仍直接写 `now`，同样的时钟偏差下清空后那几条会被同步回来。已改成一致，并补了回归测试。安卓不依赖 `qingjian-clipboard`，不受影响。
10. **（低）兜底交付的日志级别。** 结果交付和丢弃原是 debug 级，现在改 info，并在超时放弃时记一行，便于判断是修饰键一直按着还是焦点变了。

### 核对后没问题的

- **热加载**：`reload_voice` 先取消进行中的一段，再换钩子配置；`State::configure` 清掉待触发与已按住状态并补发取消；`swallowed` 和修饰键状态保留，保证已吞的按下仍能配对吞掉抬起。
- **`GetAsyncKeyState` 判断修饰键**：只在 Unicode 兜底时用，最长等到结果的 5 秒有效期，过期提示「未能写入输入框」，不会无限 Busy。用组合键触发且松开主键后一直按着 Ctrl/Alt 时会等到松开，属预期。
- **重复上屏 / 写错窗口**：TSF 取回（`take_async_commit`）与 Unicode 兜底都在 Router 线程上「谁先取走谁交付」，互斥；两条路都核对线程号与焦点控件句柄。协议新字段带 `serde(default)`，旧 DLL 不带 `accepts_commit` 时结果留着等它下次按键或轮询取走，不丢字。
- **Esc 取消**：录音和等结果期间吞掉 Esc 的按下与抬起，并真正取消；`LISTENING` 在完成、取消、焦点改变时都会复位。
- **组合键修饰位**：钩子里算出的 Alt=1、Ctrl=2、Shift=4、Win=8 与 `RegisterHotKey` 的 `MOD_*` 一致。
- **设置页查重**：语音组合键与翻译窗口组合键互相查重，也与选中文字翻译、AI 联想、符号面板翻译、剪贴板、私密输入查重；至少两个修饰键。
- **安装脚本**：翻译程序已加入编译、产物检查、`-Sign` 签名列表、安装、安装前结束进程和卸载时结束进程。
- **单实例、朗读、复制**：命名互斥体 + `FindWindowW` 唤起或收起；SAPI 标志位 3 / 11 / 19 与方案一致，XML 文本已转义，缺少对应语言声音时退回默认声音；复制用 `EM_SETSEL` + `WM_COPY`。
- 本机日志显示台式机的听写结果都是走 Unicode 兜底写进去的，DLL 日志里没有轮询取回的记录。这与 0.4.10 发布说明一致：保持打开的应用仍用升级前的旧 DLL，完全退出重开后才走 TSF 路径。不是新缺陷。

### 待定（需要你决定）

1. **TSF 编辑会话被拒时听写文字会丢。** Server 把结果交给 DLL 后就算交付了；如果 DLL 的编辑会话没被应用受理，只写日志，不会再走 Unicode 兜底。要补需要 DLL 回报成败（协议再加一个可选字段）。建议观察真机日志里有没有「语音编辑会话未受理」再决定。
2. **管理员权限窗口里的 Unicode 兜底会失败。** Server 不以管理员运行，Windows 不允许向管理员窗口注入按键，此时只能靠 TSF 路径。如果你常在管理员程序里听写，需要另想办法；否则保持现状。
3. **翻译窗口状态文字**：有道开着但实际由 AI 接口完成时（有道没配密钥），仍显示「有道翻译 · 完成」。要准确需要翻译接口回报实际用了哪家。
4. **朗读原文的语言**：原文是外文时按所选「互译语言」的声音朗读；原文是日文而互译语言选英语时会用英语声音念。可以改成按文字自动判断语言，要不要做？
5. **开始菜单「猛人翻译」快捷方式**：方案里写的是「可加」，Codex 没加。要不要加？

## 三、改了哪些文件

| 范围 | 文件 |
| --- | --- |
| 录音 | `apps/windows/server/src/audio/mod.rs`（重写：启动握手、错误通道、统计日志、格式转换）、新增 `audio/device.rs`（选设备、两段式初始化、设备名）、`audio/error.rs`（错误分类与提示）、`audio/format.rs`（格式解析与折单声道）、`audio/resample.rs`（重采样）；`apps/windows/server/Cargo.toml` 加 `Win32_Devices_FunctionDiscovery`、`Win32_UI_Shell_PropertiesSystem` 特性 |
| 语音状态机 | `apps/windows/server/src/dispatch/voice/mod.rs`、`transcriber.rs`、`tests.rs` |
| 快捷键 | `apps/windows/server/src/hotkey.rs`、`apps/windows/server/src/main.rs`（注释） |
| 语音面板 | `apps/windows/server/src/ui/voice.rs` |
| 离线识别（共享） | `crates/qingjian-voice-offline/src/config.rs`（新增 `problem()`）、`transcriber/mod.rs`、`library/mod.rs`（保留加载失败原因）。安卓行为不变：`available()` 判断条件相同，只是多了日志细节 |
| 文案 | `crates/qingjian-platform/locales/windows/` 下 7 个语言文件：新增 `mic_none`、`mic_denied`、`mic_busy`、`mic_format`、`mic_lost`、`mic_timeout`、`mic_silent`、`voice_empty`、`voice_model`、`voice_engine`、`voice_key`，改写 `voice_focus` |
| 翻译窗口 | `apps/windows/translator/src/window/mod.rs`、`translation.rs` |
| 剪贴板 | `crates/qingjian-clipboard/src/store/mod.rs`、`tests.rs` |
| 文档 | `docs/user/platforms/windows-voice-translate.md`（加「听写没有反应时」）、`docs/notes/crate-notes.md`、`docs/design/voice-and-swipe.md`、本文 |

## 四、检查结果

在仓库根目录按方案 §3 从头跑完一轮，另外加跑了剪贴板两个包、`qingjian-voice-offline` 和翻译程序的测试。语音面板改动后，又对 Server 和翻译程序重跑了格式化、锁文件编译、测试和 clippy。全部通过：

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all`（随后 `--check`） | 通过 |
| `cargo build -p qingjian-windows-translator` | 通过。`Cargo.lock` 没有变化（只加了 windows 特性开关） |
| `cargo build --locked`（Server、TSF、设置、翻译窗口） | 通过 |
| `cargo test -p qingjian-platform` | 135 项通过，其中七种语言文案键一致性检查也通过 |
| `cargo test -p qingjian-windows-server` | 200 项通过，2 项按设计忽略（需要交互桌面、需要真实麦克风）。新增：格式解析、四种采样宽度折单声道、重采样（5 种采样率、频带外抑制）、错误分类、录音打不开或中途断开时的提示、空结果的两种提示、不可用原因、按住时按其他修饰键或点鼠标会作废、Win 组合键补发遮罩键、面板长文字保留结尾 |
| `cargo test -p qingjian-windows-tsf` | 27 项通过 |
| `cargo test -p qingjian-settings` | 29 项通过，原有的联网下载模型测试按其设置忽略 |
| `cargo test -p qingjian-render` | 63 项通过 |
| `cargo test -p qingjian-clipboard -p qingjian-clipboard-app` | 55 项通过，含新增的「清空」时钟偏差回归测试 |
| `cargo test -p qingjian-voice-offline` | 8 项通过 |
| `cargo test -p qingjian-windows-translator` | 4 项通过，含新增的重试测试 |
| `cargo clippy ... --all-targets -- -D warnings`（platform、Server、TSF、设置、翻译窗口、剪贴板存储、离线识别） | 通过 |
| 实机录音（`real_microphone_records_16k_on_both_paths`，本机台式机） | 通过。系统转换路径录 1 秒得到 15680 个采样；强制走「混音格式 + 进程内转换」路径得到 16160 个采样；强制路径下第一次初始化正如预期返回 `0x88890008` 后自动退回 |

未做的：没有打包、没有安装到本机、没有在笔记本上验证；安卓、Linux、macOS 不受影响，本轮没有对它们交叉编译（共享的 `qingjian-voice-offline` 只加了方法和日志细节，`qingjian-clipboard` 安卓不依赖）。

## 五、必须在笔记本上真机验证的

自动测试覆盖不了麦克风驱动、Windows 隐私设置和各应用的输入法行为。建议用本轮代码打一个测试包（是否发 0.4.12 由你定），装到笔记本后：

1. **先完全退出**要测试的应用（微信、Chrome、Edge、记事本）再打开，确保加载新 DLL。
2. **基本听写**：在记事本里切到猛人输入法，按住右 Alt 说一句话，松开。应看到波形跳动，松开后文字写入。然后打开 `%LOCALAPPDATA%\Qingjian\logs` 当天的 `server` 日志，找 `voice capture device` 那几行，把设备名、混音格式和两行 `Initialize` 的 `hr` 发给我，用来确认走的是哪条路。
3. **各应用**：在 Chrome、Edge、微信里各试一次；再切到微软拼音试一次，应提示「请先点进输入框，并把输入法切到猛人输入法」。
4. **麦克风权限**：在「设置 → 隐私和安全性 → 麦克风」关掉「允许桌面应用访问麦克风」，按住说话，应提示「麦克风被系统拒绝」或「没有收到声音」；测完再打开。
5. **静音键**：按笔记本的麦克风静音键后说话，松开后应提示「没有收到声音」。
6. **没说话**：按住右 Alt 两秒不说话，应提示「没有听清」或「没有收到声音」，不再无声无息。
7. **中途拔麦**（有外接麦克风时）：录音中拔掉，应立即提示「录音中断」。
8. **误触发**：按住右 Alt 时点一下鼠标、按住 Win 后按 Shift+S 截图，都不应开始录音；Win+Shift+S 截图正常。
9. **离线识别**：如果用本机离线识别，确认模型已下载；如果提示「离线识别引擎加载失败」，把日志里 `offline speech engine unavailable` 那行发给我。
10. **翻译窗口**：在缩放 125% 或 150% 的屏幕上按 Ctrl+Alt+F，窗口应完整显示、按钮不重叠；断网翻译失败后联网，点「翻译」应能重试。
11. **剪贴板**：两台设备同步后在一台上「全部清空」，另一台同步回来后记录不应重新出现。
