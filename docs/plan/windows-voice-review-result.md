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

## 六、笔记本日志复核（2026-11-20，0.4.13）

用户在笔记本上装 0.4.13 后，听写会写入「ㄱ.」，随后发来日志：

- 设备为「本机麦克风（适用于数字麦克风的英特尔® 智音技术）」，混音格式 **48000 Hz × 4 声道 Float32**。0.4.11 在这台机器上每次都报 `初始化录音失败 … 0x88890008`，证实第一节的根因，0.4.12 起已修复：系统转换路径 `hr=0x00000000`，录音正常开始，每段数百个包、没有静音包标记。
- 但每段录音（2.5～12.7 秒）的原始峰值都是 `0.0000`（−90.3 dBFS，即 1 个最小刻度），也就是麦克风交出的是数字静音。离线模型自动判断语言，对静音猜出「ㄱ.」。常见原因是 Windows 隐私设置不允许桌面应用访问麦克风（此时系统允许打开、但只送静音）、麦克风静音键，或输入音量为 0；需用户在笔记本上检查。
- 程序侧加了两道保护：整段峰值低于约 −66 dBFS 时丢弃识别结果并提示「没有收到声音」及检查步骤；结果里没有汉字、假名、韩文音节、字母或数字（只有「ㄱ」这类单独字母与标点）时也不写入。录音线程收到包就上报电平（含 0），Router 才能区分「纯静音」与「没有电平」。
- 设置程序「关于」页新增「日志」一栏（打开日志目录 / 打包到桌面），与「高级」页相同。

用户随后确认：笔记本的麦克风静音键处于开启状态，关闭后 0.4.13 听写正常。上述两道保护与「关于」页日志入口尚未发布。

## 七、翻译快捷键带入选中文字（2026-11-20，未发布）

用户反馈 Ctrl+Alt+F 选中文字后按并不翻译，希望有更顺手的单键。查到两处原因：

- **Ctrl+Alt+F 本来就不读选区**：它被 Server 的全局钩子拦下，DLL 收不到按键，方案里也只做了「打开窗口」。现在与新增的「双击右 Ctrl」走同一条路：借前台会话的下一次空闲同步读选区，带进翻译窗口立即翻译。用户选定双击右 Ctrl（不选单个 F 键，F5 等会在所有程序里被拦）。
- **Ctrl+Alt+T（原地翻译选中文字）在「只开有道、不开 AI」时完全不工作**：Windows Server 在 `[predict] enabled = false` 时把云服务换成空实现，有道没有接入，判断条件又看的是云联想开关。现在开了有道就接成 Core 的显式翻译通道（`set_translation_predictor`），判断改看 `translation_enabled()`。
- 另修正 `ShortcutConfig.translator` 的注释（原误写成「组句中按需调用 AI」）。

改动：协议 `SyncMode.reads_selection`、`ModeSync.read_selection`（均可缺省）；DLL `com/edit/selection.rs`（`SelectionUse`）、`com/poll/mod.rs`、`client/`；Server 新增 `dispatch/translator_window/`，改 `hotkey.rs`（双击识别）、`dispatch/{message,translate,rescore,reload,config,voice,status}`、`ui/mod.rs`（`--stdin` 启动）、`main.rs`；翻译程序新增 `window/incoming.rs`（`WM_COPYDATA`），`Cargo.toml` 加 `Win32_System_DataExchange`；配置 `[shortcut] translator_double_ctrl`（缺省开）及模板；设置程序「快捷键」页开关与说明、「恢复默认」；七种语言文案；`keys.md` 与 Windows 语音翻译用户文档。

需真机验证：在记事本、浏览器、微信里选中文字后双击右 Ctrl / 按 Ctrl+Alt+F，窗口应带着文字出来并翻译；没选中时只开窗口；窗口在前台时双击收起；只开有道时 Ctrl+Alt+T 应在原地出译文。更新后需完全退出并重开应用（新 DLL 才会读选区）。

## 八、本地 0.4.14 测试包复测（2026-11-20）

用户装本地 0.4.14 测试包后反馈两点并附日志：

1. **Ctrl+Alt+T 一直停在「翻译中…」**：日志显示有道 171 ms 就返回译文，但 `poll_prediction` 第一行要求 AI 云联想开着才取结果，只开有道时译文永远取不出来。已改为云联想或翻译通道任一开着就取，补测试 `selection_translation_works_with_only_the_translation_channel`。
2. **网页正文选中后，双击右 Ctrl / Ctrl+Alt+F 只开空窗口**：DLL 日志每次 `翻译窗口读选区会话未受理: 0x80004005`——浏览器网页正文不是可编辑文档，拒绝编辑会话，输入法读不到。用户选定「辅助接口优先，读不到再模拟复制」，实现见设计文档；协议 `Selection` 加可缺省的 `unreadable`，DLL 要不到会话时立即回报，不再等 0.7 秒超时。

开发中在本机实测时出了两次问题，均已修复并如实告知用户：

- 第一次模拟复制后原剪贴板没放回（放回时打开剪贴板只重试约 0.2 秒，猛人剪贴板正在读新内容），用户当时剪贴板里的内容被测试文字覆盖，需从剪贴板历史找回。已改为保存 0.3 秒（打不开就整步不做）、读取 1 秒、放回 3 秒的分步限时；20 万字文字作原内容时实测完整放回。
- 放回时曾标 `ExcludeClipboardContentFromMonitorProcessing`，被猛人剪贴板当成密码私密发送给已配对设备（用户看到「收到隐私密码」）；发出去的只有测试用的哨兵文字。已去掉该标记。
- 实测还确认：Win32 原生 EDIT（含 WinForms 文本框）在 UI Automation 里不提供 TextPattern，这类输入框靠 TSF 读；模拟 Ctrl+Insert 在 WinForms 文本框里能取到选中文字。测试时弹出了多个临时窗口打扰到用户，会弹窗、动剪贴板的测试已删除，之后只在用户同意时做。

待真机验证：Chrome / Edge 网页正文、微信聊天记录、Word 里选中文字后双击右 Ctrl；确认剪贴板原内容在翻译后仍在、不再出现「收到隐私密码」。

## 九、第二轮本地测试包复测（2026-11-20）

用户反馈与对应处理（均按日志核对）：

1. **微信输入框、翻译窗口自己的输入框、猛人快传里双击右 Ctrl 读不到选中文字**：这些程序经兼容层接入文本服务，输入法读选区时系统回「没选中」（日志 `chars=0 unreadable=false`），而上一版对「没选中」只问辅助接口、不模拟复制。现在一律继续取，`TranslatorOpen::Probe` 去掉了「是否复制」开关。
2. **Ctrl+Alt+T 在微信、WPS、猛人快传里无反应**：同一原因，现在读到空选区就改用辅助接口 / 模拟复制，取到后让 DLL 进评审态。
3. **「收到隐私密码」仍会弹出**：猛人剪贴板把 `CanIncludeInClipboardHistory = 0`、`CanUploadToCloudClipboard = 0` 也当成密码标记，上一版放回原剪贴板时仍带着这两个标记，于是用户真实的剪贴板原内容被当作密码私密发给了已配对设备（日志里三次放回）。现在放回时不加任何标记；原剪贴板本身是密码类内容时整步不模拟复制。
4. **新增双击右 Shift 原地翻译**（用户选定），配置 `[shortcut] translate_double_shift`，设置页可关。
5. **ChatGPT 桌面版回车是插入不是替换**：评审开始时记下选区范围，接受时直接替换它。
6. 测试中发现并修复：`ensure_focus` 换焦点会清掉在等的读选区请求。

待真机验证：微信、WPS、猛人快传、翻译窗口、ChatGPT 桌面版里双击右 Ctrl / 双击右 Shift / Ctrl+Alt+T；确认不再出现「收到隐私密码」；ChatGPT 里回车替换原文。用户同意由 Claude 控制电脑测试，微信里不按回车。

## 十、第三轮本地测试包复测（2026-11-20）

用户在微信、WPS、猛人快传、ChatGPT、Claude、Obsidian 复测后反馈原地翻译卡在「翻译中…」、双击右 Shift 无反应、ChatGPT 回车仍是插入。按用户要求先读日志、自测确认原因再改：

| 现象 | 确认的原因 | 修改 |
| --- | --- | --- |
| 微信卡「翻译中…」，Esc 关的是微信 | 微信进程 20:35 启动、仍挂着上一版 DLL，不认 `start_review`；Server 当作已送达，评审没人接、也不超时 | DLL 声明 `in_place_review`，不认的旧 DLL 改开翻译窗口；`start_review` 要等 DLL 轮询确认，2 秒没确认就收起 |
| WPS 双击右 Shift 无反应、组合键卡「翻译中…」 | WPS 外框窗口与编辑区分属两个 wps.exe，DLL 的「前台」检查永远不成立，从不做空闲同步 | 键盘焦点在本线程也算前台 |
| 记事本里 Ctrl+Alt+T 的评审一开始就结束 | 主键被输入法吃掉，应用把单独松开的 Alt 当成进菜单访问键模式、夺焦，DLL 落定结束评审（诊断日志：开始后 130 ms DLL 发 HideCandidates） | DLL 吃掉带 Alt / Win 的保留键后补发遮罩键 |
| Claude / ChatGPT 双击右 Shift 无反应；ChatGPT 回车仍插入 | 两者进程 18:04 / 19:13 启动、仍是旧 DLL；之前卡住的评审一直没清，挡住之后所有请求 | 同上（确认 + 超时）；需重启这两个程序加载新 DLL |
| Obsidian 双击右 Shift 卡「翻译中…」 | 进程 20:46 启动、上一版 DLL 把原地读选区当成翻译窗口读，不进评审 | 同上 |
| 猛人快传读不到选中文字 | Flutter 程序，不认 Ctrl+Insert | Ctrl+Insert 没复制到再试 Ctrl+C，终端与带终端的编辑器除外 |
| 失焦补同步会抹掉「能读选区」并吞掉进评审 | `sync_mode_from_server` 带 `false` 且不处理附加项 | 能力只记不清；请求只交给空闲轮询的同步 |

另：兜底取文字时带上光标（取不到用鼠标）位置，候选窗摆在它下面；没取到文字、翻译服务没开时语音面板给出提示；`ensure_focus` 不再把调试日志留在代码里。`apps/windows/server/tests/cloud_usage.rs` 单独运行也约三分之一失败（3 秒等本地假服务器，修改前已出现，不开有道、与本轮改动无关），留作后续。

测试提醒：已开着的程序要完全退出（微信要从托盘退出）再打开才会载入新 DLL；粘滞键提示由 Windows「设置 → 辅助功能 → 键盘」关闭。

## 十一、第四轮：装 22:32 测试包后的实测（2026-11-20）

按用户要求先读日志、自测确认原因再改。Claude 控制电脑只能测 Ctrl+Alt+T（走 TSF 保留键）；双击右 Ctrl / 右 Shift、Ctrl+Alt+F 是 Server 低级钩子，钩子故意不理模拟按键，这几项靠用户 22:38–23:08 自己按的日志核对。

已确认正常（22:32 包）：

| 程序 | 结果 |
| --- | --- |
| WPS | Ctrl+Alt+T：DLL 读到空选区 → Ctrl+Insert 取到 22 字 → 评审 → 回车替换原文 |
| 猛人学习（Python + WebView2） | 输入框 Ctrl+Alt+T 回车替换；用户双击右 Ctrl 带入 24 字、双击右 Shift 后 Esc 保留原文 |
| ChatGPT（Codex） | Ctrl+Alt+T 回车**替换**原文（上一轮的「插入」已修好） |
| 微信 | 用户双击右 Shift（Ctrl+Insert 13 字、3 字）、双击右 Ctrl 均取到文字 |
| 记事本 | 用户双击右 Shift 回车替换、双击右 Ctrl 带入 22 字 |
| 翻译窗口自己 | 原地翻译经 Ctrl+Insert 取到 8 字；双击右 Ctrl 在翻译窗口里是收起（设计如此） |

本轮确认原因并修改：

| 现象 | 确认的原因 | 修改 |
| --- | --- | --- |
| 猛人快传里双击右 Ctrl / 右 Shift 全部「没选中文字」（22:38 连续 11 次） | Server 模拟 Ctrl+Insert / Ctrl+C 只给虚拟键码、扫描码为 0；Flutter 按扫描码认键，不当 Ctrl+C。对照实验：同一选区，不带扫描码不复制，带扫描码复制成功 | `selection/copy.rs` 模拟复制带扫描码 |
| 记事本 Ctrl+Alt+T 评审仍一开始就结束 | 上一轮补发的 0xE8 确实发出（低级钩子记到），但新版记事本（WinUI）不认，松开 Alt 后 16 ms 焦点移到访问键层；0x07、0xFF、F24 同样无效，补按 Ctrl（三种按法）有效 | DLL `com/key/mask.rs`：Alt 按着时再补按一下没按着的那侧 Ctrl。Alt 按着时它以系统键消息到达，TSF 不交给按键接收器（日志核实没有转发），评审不受影响；最后是松开状态，不会留下 Ctrl 按着 |
| 同类隐患 | Server 全局快捷键（Ctrl+Alt+F、按住右 Alt 说话）的遮罩只发 0xE8；猛人剪贴板粘贴回原窗口的 Ctrl+V 也不带扫描码，往 Flutter 程序粘贴会同样失灵 | `hotkey.rs` 遮罩与 DLL 同一做法；`apps/clipboard/src/system/windows/paste.rs` 带扫描码 |

看到但没改：双击右 Shift 时「单击 Shift 切中英」会让输入法连切两次（英 → 中），净结果不变，状态条可能闪一下；猛人学习的网页内核在第一次被辅助接口询问时才打开无障碍，第一次读不到，会退到模拟复制。

测试副作用（已如实告知用户）：电脑控制工具输入文字走剪贴板，剪贴板历史里多了几条 `good morning my friend`，快传对照实验复制了用户消息里的一个词；误点把猛人学习的「峰级分析」文件夹展开、Codex 切到「站点」页，前者已收起；猛人学习搜索框、Codex 输入框里的测试文字已清空；WPS 新建的「文字文稿1」里留着「早上好我的朋友」（未保存）。测试脚本里用 Esc 会被当成「停止 Claude」，两次中断后改用鼠标点击。

待真机验证（新包）：猛人快传里双击右 Ctrl / 右 Shift / Ctrl+Alt+T；从猛人剪贴板往快传粘贴；记事本 Ctrl+Alt+T；WPS、Obsidian、ChatGPT 里双击右 Shift。

## 十二、翻译窗口：朗读语速、能读的语言、按钮缩小（2026-11-20，用户要求）

- 朗读用的是 Windows 经典 SAPI 声音，不是有道。本机只装了中文 Huihui、英文 Zira（系统新语音 OneCore 里也只有三个中文），日、韩、法、德、西、俄、越都读不了；以前读不了时退回默认中文声音硬读，读出来是错的。
- 「朗读译文」旁加「语速」按钮：0.5× / 0.75× / 1.0× / 1.25× / 1.5× / 2.0×，换成 SAPI 语速 -6 / -3 / 0 / 2 / 4 / 6（语速 10 约 3 倍，按对数取整），原文译文共用，存 `translator-speech-speed`。网上没查到有道翻译朗读倍速的官方说明，档位按常见播放器做。
- 打开窗口时查一次本机各语言有没有声音；底部小字列出能读的语言，当前外语读不了时点明，读不了的那边朗读按钮变灰、不再硬读。
- 按钮高 34 → 28，宽度按文字量（各语言文案长短不一，写死会截断）。

## 十三、语音后删除键、空格失灵；翻译窗口细节（2026-11-20）

**现象**：在 Claude 桌面版里按住右 Alt 听写，文字上屏后光标还在闪，字母能打（经输入法写入），删除键、空格没反应，要点一下输入框才恢复。

**确认的原因**：用只读记录器录下真实一次听写的按键与焦点：

```
右 Alt 按下 → 0.18 秒后 Server 补发遮罩（0xE8、左 Ctrl）→ 30 ms 后系统自动连发「右 Alt 按下」→ 5 秒后松开
→ 焦点从「提示词」输入框跳到菜单项「文件」（MenuItemView）→ 删除、空格被菜单吃掉 → 点击后焦点回输入框
```

Electron 程序把遮罩之后的连发当成又单独按了一次 Alt，松开就进菜单模式。在 Claude 里模拟复现：补遮罩后连发 5 秒必进菜单；松开前再补遮罩、或不连发，都不进。只连发零点几秒复现不了（之前的模拟因此漏判）。

**修改**：`hotkey.rs` 说话期间吞掉触发键的自动连发（松开照常放行），测试 `voice_trigger_flow` 补断言。没选「松开前再补遮罩」：那要先吞掉真实的松开再补发，补发被系统拦下（前台是管理员程序）时 Alt 会卡住。

**翻译窗口**：语速按钮只显示倍数，悬停说明；互译语言、背景色、语速三个下拉菜单改成自绘（`window/popup.rs`），底色、文字、悬停高亮与边框随窗口配色；三处均截图核对。
