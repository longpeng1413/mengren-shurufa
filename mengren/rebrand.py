"""猛人输入法换牌：把官方青简源码里用户看得见的名字、图标、安卓包名换成猛人输入法。

用法（仓库根目录，任何 Python 3.8+）：
    python mengren/rebrand.py          # 换牌
    python mengren/rebrand.py --check  # 只检查是否都已换好，不改文件（有没换的返回 1）

为什么用脚本而不是手改：合并官方更新时，凡是这里改过的文件起冲突，一律先取官方版本，
再跑一遍本脚本就换回来了，不用逐行手工解决冲突。脚本可以重复运行，结果不变。

只换用户看得见的东西：软件名、图标、安卓 applicationId、默认关闭检查官方更新。
内部名字（crate 名、Kotlin 包名、数据目录 Qingjian、TSF 的 CLSID）一律不动，减少与上游的冲突。
注明数据来源的地方（「青简自建」「读音取自青简词库」）是对原作者的署名，保留不改。
"""
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / 'mengren' / 'brand'
ANDROID_RES = ROOT / 'apps/android/app/src/main/res'

# ---- 文字替换：(文件, [(旧, 新), ...])。旧字符串里的 \n 会按文件实际的换行符匹配。
TEXT = [
    # 安卓：应用名与界面文字；空格键上写「猛人」，五个字太挤
    ('apps/android/app/src/main/res/values/strings.xml', [('青简翻译', '猛人翻译'), ('青简', '猛人输入法')]),
    ('apps/android/app/src/main/kotlin/app/qingjian/inputmethod/keyboard/layout/Key.kt',
     [('Key("青简", weight)', 'Key("猛人", weight)')]),
    # 安卓包名：和官方青简分开（清单里的 authorities 都跟着 ${applicationId} 走）
    ('apps/android/app/build.gradle.kts',
     [('applicationId = "app.qingjian.inputmethod"', 'applicationId = "com.mengren.shurufa"')]),
    # Windows：系统「语言 / 输入法」列表里显示的名字
    ('apps/windows/tsf/src/com/mod.rs',
     [('SERVICE_DESCRIPTION: &str = "青简"', 'SERVICE_DESCRIPTION: &str = "猛人输入法"')]),
    # Windows 输入法列表和 Server 必须使用同一张彩色图，否则 Server 会覆盖注册图标
    ('apps/windows/tsf/src/com/registry/icon.rs', [
        ('resources/qingjian-mono-dark.ico', 'resources/mengren-profile.ico'),
        ('resources/qingjian-mono-light.ico', 'resources/mengren-profile.ico'),
    ]),
    ('apps/windows/server/src/ui/taskbar_icon.rs', [
        ('resources/qingjian-mono-dark.ico', 'resources/mengren-profile.ico'),
        ('resources/qingjian-mono-light.ico', 'resources/mengren-profile.ico'),
    ]),
    # Windows 安装包：名字、开始菜单、完成提示
    ('apps/windows/installer/qingjian.iss', [
        ('#define AppName "青简"', '#define AppName "猛人输入法"'),
        ('#define Publisher "青简"', '#define Publisher "猛人"'),
        ('DefaultDirName={autopf}\\Qingjian', 'DefaultDirName={autopf}\\mengrenshurufa'),
        ('青简才会在所有应用里生效', '猛人输入法才会在所有应用里生效'),
        ('{group}\\青简设置"', '{group}\\猛人输入法设置"'),
        ('{group}\\青简旧版设置"', '{group}\\猛人输入法旧版设置"'),
        ('{group}\\卸载青简"', '{group}\\卸载猛人输入法"'),
        ('{commonstartup}\\青简 Server"', '{commonstartup}\\猛人输入法 Server"'),
    ]),
    # 设置程序（三平台共用）的简体中文界面
    ('apps/settings/locales/zh-Hans.toml', [
        ('window_title = "青简设置"', 'window_title = "猛人输入法设置"'),
        ('app_name = "青简"', 'app_name = "猛人输入法"'),
        ('在青简里切换中文与英文', '在猛人输入法里切换中文与英文'),
        ('clipboard = "青简剪贴板"', 'clipboard = "猛人剪贴板"'),
        ('file_name = "青简自定义短语-', 'file_name = "猛人输入法自定义短语-'),
        ('file_name = "青简生词本-', 'file_name = "猛人输入法生词本-'),
        ('file_name = "青简设置-', 'file_name = "猛人输入法设置-'),
        ('file_filter = "青简设置"', 'file_filter = "猛人输入法设置"'),
    ]),
    ('apps/settings/src/support/mod.rs', [('Self::Clipboard => "青简剪贴板"', 'Self::Clipboard => "猛人剪贴板"')]),
    ('apps/clipboard/locales/zh-Hans.toml', [('tip = "青简剪贴板"', 'tip = "猛人剪贴板"')]),
    # 猛人更新源已接入独立签名清单，默认检查开启
    ('crates/qingjian-platform/src/config/update/mod.rs', [('check: false,', 'check: true,')]),
    ('crates/qingjian-platform/src/config/mod.rs', [('\ncheck = false\n# 渠道', '\ncheck = true\n# 渠道')]),
]

# ---- 整个文件覆盖：(目标, 内容)
ADAPTIVE_ICON = '''<?xml version="1.0" encoding="utf-8"?>
<!-- 猛人输入法：由 mengren/rebrand.py 生成，图标见 mengren/brand/android -->
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/mengren_launcher_background" />
    <foreground android:drawable="@mipmap/mengren_launcher_foreground" />
    <monochrome android:drawable="@mipmap/mengren_launcher_monochrome" />
</adaptive-icon>
'''
BRAND_DRAWABLE = '''<?xml version="1.0" encoding="utf-8"?>
<!-- 猛人输入法：由 mengren/rebrand.py 生成。键盘工具栏、首页、关于页的品牌标，指向 drawable-nodpi/mengren_brand.png -->
<bitmap xmlns:android="http://schemas.android.com/apk/res/android"
    android:src="@drawable/mengren_brand"
    android:filter="true"
    android:gravity="fill" />
'''
COLORS = '''<?xml version="1.0" encoding="utf-8"?>
<!-- 猛人输入法：由 mengren/rebrand.py 生成。自适应图标的背景色（赤焰黑金的底色） -->
<resources>
    <color name="mengren_launcher_background">#1A0503</color>
</resources>
'''
LAUNCHER_FALLBACK = '''<?xml version="1.0" encoding="utf-8"?>
<!-- 猛人输入法：旧前景资源也指向既定天马猛素材，避免回退时出现竹简。 -->
<bitmap xmlns:android="http://schemas.android.com/apk/res/android"
    android:src="@mipmap/mengren_launcher_foreground"
    android:filter="true"
    android:gravity="fill" />
'''
WRITE = [
    ('apps/android/app/src/main/res/drawable/ic_launcher_foreground.xml', LAUNCHER_FALLBACK),
    ('apps/android/app/src/main/res/mipmap-anydpi-v26/ic_launcher.xml', ADAPTIVE_ICON),
    ('apps/android/app/src/main/res/drawable/ic_brand.xml', BRAND_DRAWABLE),
    ('apps/android/app/src/main/res/values/mengren_colors.xml', COLORS),
]

# ---- 图标复制：(mengren/brand 下的源, 目标)
COPY = [
    ('mengren-512.png', 'apps/settings/assets/mengren.png'),
    ('windows/mengren-profile.ico', 'apps/windows/tsf/resources/qingjian.ico'),  # 输入法、Server、设置、剪贴板、安装包共用
    ('windows/mengren-profile.ico', 'apps/windows/tsf/resources/mengren-profile.ico'),
    ('windows/mengren-mono-dark.ico', 'apps/windows/tsf/resources/qingjian-mono-dark.ico'),
    ('windows/mengren-mono-light.ico', 'apps/windows/tsf/resources/qingjian-mono-light.ico'),
    ('windows/mengren-profile.ico', 'apps/clipboard/resources/tray-dark.ico'),
    ('windows/mengren-profile.ico', 'apps/clipboard/resources/tray-light.ico'),
    ('android/drawable-nodpi/mengren_brand.png', 'apps/android/app/src/main/res/drawable-nodpi/mengren_brand.png'),
] + [(f'android/mipmap-{d}/{n}', f'apps/android/app/src/main/res/mipmap-{d}/{n}')
     for d in ('mdpi', 'hdpi', 'xhdpi', 'xxhdpi', 'xxxhdpi')
     for n in ('mengren_launcher_foreground.png', 'mengren_launcher_monochrome.png')]


def read(path):
    raw = path.read_bytes()
    bom = raw.startswith(b'\xef\xbb\xbf')
    text = raw[3:].decode('utf-8') if bom else raw.decode('utf-8')
    return text, bom


def write(path, text, bom):
    data = text.encode('utf-8')
    path.write_bytes((b'\xef\xbb\xbf' if bom else b'') + data)


def main(check_only):
    problems, changed = [], []
    for rel, rules in TEXT:
        path = ROOT / rel
        if not path.exists():
            problems.append(f'找不到文件：{rel}（上游可能改了位置）')
            continue
        text, bom = read(path)
        nl = '\r\n' if '\r\n' in text else '\n'
        new = text
        for old, rep in rules:
            old_n, rep_n = old.replace('\n', nl), rep.replace('\n', nl)
            if old_n in new:
                new = new.replace(old_n, rep_n)
            elif rep_n not in new:
                problems.append(f'{rel}：既找不到「{old}」也没有「{rep}」（上游可能改了这句，要更新脚本）')
        if new != text:
            changed.append(rel)
            if not check_only:
                write(path, new, bom)

    for rel, content in WRITE:
        path = ROOT / rel
        current = path.read_text(encoding='utf-8') if path.exists() else None
        if current is None or current.replace('\r\n', '\n') != content:
            changed.append(rel)
            if not check_only:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding='utf-8', newline='\n')

    for src, dst in COPY:
        s, d = BRAND / src, ROOT / dst
        if not s.exists():
            problems.append(f'缺少图标源文件：mengren/brand/{src}')
            continue
        if not d.exists() or d.read_bytes() != s.read_bytes():
            changed.append(dst)
            if not check_only:
                d.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(s, d)

    verb = '需要换' if check_only else '已换'
    print(f'{verb} {len(changed)} 个文件' + ('' if not changed else '：\n  ' + '\n  '.join(changed)))
    for p in problems:
        print('⚠ ' + p)
    if problems or (check_only and changed):
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main('--check' in sys.argv[1:]))
