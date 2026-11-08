"""核对 Windows/Android 当前运行入口的品牌、图标与导出名；不改内部身份或上游署名。"""

from pathlib import Path
import json
import runpy
import subprocess
import sys
import tomllib
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent


def main():
    subprocess.run([sys.executable, "-X", "utf8", ROOT / "mengren/rebrand.py", "--check"], check=True, cwd=ROOT)
    rebrand = runpy.run_path(str(ROOT / "mengren/rebrand.py"))
    icons = []
    for source, target in rebrand["COPY"]:
        assert (ROOT / target).read_bytes() == (ROOT / "mengren/brand" / source).read_bytes(), target
        icons.append(target)
    for folder in ["apps/settings/locales", "apps/clipboard/locales"]:
        for path in (ROOT / folder).glob("*.toml"):
            data = tomllib.loads(path.read_text(encoding="utf-8-sig"))
            visible = [data.get("common", {}).get(k, "") for k in ("app_name", "window_title")]
            visible.append(data.get("tray", {}).get("tip", ""))
            assert all("Qingjian" not in value and "青简" not in value and "青簡" not in value for value in visible), path
    for locale in ["values", "values-en"]:
        xml = ROOT / f"apps/android/app/src/main/res/{locale}/strings.xml"
        strings = {node.attrib["name"]: node.text or "" for node in ET.parse(xml).getroot() if node.tag == "string"}
        name = strings.get("app_name", "猛人输入法")
        assert "青简" not in name and "Qingjian" not in name
        for key in ["clipboard_code_fill", "clipboard_code_masked", "clipboard_sensitive_fill"]:
            assert strings[key], key
    exports = [
        "apps/android/app/src/main/kotlin/app/qingjian/inputmethod/settings/page/AdvancedActivity.kt",
        "apps/settings/src/system/action.rs",
        "apps/windows/settings/src/panel/controls.rs",
    ]
    for name in exports:
        text = (ROOT / name).read_text(encoding="utf-8-sig")
        assert "mengren-shurufa-logs" in text and "qingjian-logs" not in text, name
    for name in ["apps/settings/build.rs", "apps/clipboard/build.rs", "apps/windows/server/build.rs", "apps/windows/settings/build.rs"]:
        text = (ROOT / name).read_text(encoding="utf-8-sig")
        assert '.set("ProductName", "猛人输入法")' in text, name
        assert '.set("FileDescription", "猛人' in text, name
    print(json.dumps({"runtime_icon_targets_verified": len(icons), "exports_verified": len(exports),
        "windows_version_resources_verified": 4, "attribution_and_internal_ids_preserved": True}, ensure_ascii=False))


if __name__ == "__main__":
    main()
