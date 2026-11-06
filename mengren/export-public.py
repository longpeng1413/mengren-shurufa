"""从当前代码导出可公开源码；不携带开发仓库的 Git 历史和本机资料。"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
PRIVATE = {"mengren/开发笔记.md", "mengren/设计文档-v0.2.md"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    output = Path(args.output).resolve()
    if output.exists() and any(output.iterdir()):
        raise SystemExit("输出目录必须不存在或为空，避免混入旧文件")
    output.relative_to(ROOT / "target")
    output.mkdir(parents=True, exist_ok=True)
    paths = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=ROOT).decode("utf-8").split("\0")
    manifest = {}
    for name in sorted(set(paths)):
        if not name or name in PRIVATE or name.startswith(("mengren/private/", "mengren/design/", "mengren/安装包/")):
            continue
        source = ROOT / name
        if not source.is_file():
            continue
        source.resolve().relative_to(ROOT)
        destination = output / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
        manifest[name] = hashlib.sha256(destination.read_bytes()).hexdigest()
    (output.parent / (output.name + "-manifest.json")).write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"已导出 {len(manifest)} 个文件到 {output}")


if __name__ == "__main__":
    main()
