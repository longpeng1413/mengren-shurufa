"""发布经检查的猛人源码和两端安装包；原开发仓库历史始终留在私有库。"""

import argparse
import ctypes
from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parent.parent
REPO = "longpeng1413/mengren-shurufa"
PUBLIC_ID = 1447865709


def run(*args, cwd=ROOT, capture=False):
    result = subprocess.run([str(arg) for arg in args], cwd=cwd, check=True,
                            stdout=subprocess.PIPE if capture else None, encoding="utf-8", errors="replace")
    return result.stdout.strip() if capture else None


def gh_json(*args):
    return json.loads(run("gh", "api", *args, capture=True))


def find_release(version):
    # GitHub 的按标签查询不返回未发布草稿；草稿必须从仓库发布列表按 tag_name 定位。
    releases = gh_json(f"repos/{REPO}/releases?per_page=100")
    return next((item for item in releases if item["tag_name"] == f"v{version}"), None)


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--notes", type=Path, required=True)
    parser.add_argument("--gitleaks", type=Path, required=True)
    parser.add_argument("--publish", action="store_true")
    parser.add_argument("--sdk", type=Path, help="Android SDK；缺省读本机 local.properties")
    args = parser.parse_args()
    version = args.version
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise SystemExit("只发布 X.Y.Z 正式版")
    for relative in ["apps/android/core/Cargo.toml", "apps/clipboard/Cargo.toml", "apps/settings/Cargo.toml", "apps/windows/server/Cargo.toml", "apps/windows/tsf/Cargo.toml", "apps/windows/settings/Cargo.toml"]:
        if f'version = "{version}"' not in (ROOT / relative).read_text(encoding="utf-8"):
            raise SystemExit(f"产品版本不一致：{relative}")
    repo = gh_json(f"repos/{REPO}")
    if repo["id"] != PUBLIC_ID or repo["private"]:
        raise SystemExit("目标必须是已确认的独立公开仓库")
    private = ROOT / "mengren/private"
    state_path = private / "public-release-state.json"
    if state_path.exists():
        state = json.loads(state_path.read_text(encoding="utf-8"))
    else:
        initial = ROOT / "target/privacy-audit/public-readiness.json"
        receipt = json.loads(initial.read_text(encoding="utf-8-sig"))
        manifest = json.loads((initial.parent / receipt["sha256_manifest"]).read_text(encoding="utf-8-sig"))
        state = {"commit": receipt["commit"], "manifest": manifest}
    pending_path=private / "pending-release.json"
    work = ROOT / "target/release-work" / f"{version}-{uuid.uuid4().hex[:8]}"
    export = work / "source"
    run(sys.executable, ROOT / "mengren/export-public.py", "--output", export)
    manifest = json.loads((work / "source-manifest.json").read_text(encoding="utf-8"))
    public = ROOT / "target/public-release"
    if not (public / ".git").exists():
        run("git", "clone", "--no-tags", f"https://github.com/{REPO}.git", public)
    if run("git", "status", "--porcelain", cwd=public, capture=True):
        raise SystemExit("公开工作目录存在未提交变更，先检查，不能覆盖")
    if run("git", "remote", "get-url", "origin", cwd=public, capture=True) != f"https://github.com/{REPO}.git":
        raise SystemExit("公开工作目录远端不匹配")
    run("git", "fetch", "origin", "main", cwd=public)
    run("git", "merge-base", "--is-ancestor", state["commit"], "origin/main", cwd=public)
    remote_head=run("git","rev-parse","origin/main",cwd=public,capture=True)
    if pending_path.exists():
        pending=json.loads(pending_path.read_text(encoding="utf-8"))
        if pending.get("version")==version and pending.get("commit")==remote_head:
            state=pending
    remote_changed = set(run("git", "diff", "--name-only", "-z", state["commit"], "origin/main", cwd=public, capture=True).split("\0")) - {""}
    local_changed = {name for name in set(state["manifest"]) | set(manifest) if state["manifest"].get(name) != manifest.get(name)}
    if remote_changed & local_changed:
        raise SystemExit("GitHub 与本机同时修改了这些文件，需要先合并：" + ", ".join(sorted(remote_changed & local_changed)))
    run("git", "merge", "--ff-only", "origin/main", cwd=public)
    for name in sorted(local_changed):
        destination = public / name
        destination.resolve().relative_to(public.resolve())
        if name in manifest:
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(export / name, destination)
        elif destination.is_file():
            destination.unlink()
    run(args.gitleaks.resolve(), "dir", public, "--config", public / ".gitleaks.toml", "--redact=100", "--no-banner")
    if local_changed:
        run("git", "add", "--all", cwd=public)
    staged = subprocess.run(["git", "diff", "--cached", "--quiet"], cwd=public).returncode
    if staged not in (0,1):
        raise SystemExit("无法检查待提交源码")
    if staged == 1:
        run("git", "-c", "user.name=longpeng1413", "-c", "user.email=295510274+longpeng1413@users.noreply.github.com", "commit", "-m", f"chore(release): 发布 v{version} 正式版", cwd=public)
    commit = run("git", "rev-parse", "HEAD", cwd=public, capture=True)
    dist = ROOT / "target/release-dist" / version
    dist.mkdir(parents=True, exist_ok=True)
    packages = [("android", "arm64", "猛人输入法-安卓-*.apk", f"mengren-shurufa-android-v{version}.apk"),
                ("windows", "x86_64", "猛人输入法-Windows-*-setup.exe", f"mengren-shurufa-windows-x64-v{version}-setup.exe")]
    assets = []
    sdk = args.sdk
    if sdk is None:
        line = next(line for line in (ROOT / "apps/android/local.properties").read_text(encoding="utf-8").splitlines() if line.startswith("sdk.dir="))
        sdk = Path(line.split("=", 1)[1].replace("\\:", ":").replace("\\\\", "\\"))
    tools = sorted((sdk / "build-tools").iterdir(), key=lambda path: path.name)[-1]
    for platform, cpu, pattern, name in packages:
        candidates = sorted((ROOT / "mengren/安装包").glob(pattern), key=lambda path: path.stat().st_mtime, reverse=True)
        if not candidates:
            raise SystemExit(f"缺少 {platform} 安装包")
        if platform == "android":
            badging = run(tools / "aapt2.exe", "dump", "badging", candidates[0], capture=True)
            if f"versionName='{version}'" not in badging or "name='com.mengren.shurufa'" not in badging or "application-debuggable" in badging:
                raise SystemExit("APK 产品版本、包名或正式包标记不匹配")
            certificates = run("java", "-jar", tools / "lib/apksigner.jar", "verify", "--print-certs", candidates[0], capture=True)
            expected = (ROOT / "mengren/android-signing-cert.sha256").read_text().strip()
            if f"certificate SHA-256 digest: {expected}" not in certificates:
                raise SystemExit("APK 签名与现装版本不一致，禁止发布")
        else:
            if os.name != "nt":
                raise SystemExit("Windows 安装包校验需要在 Windows 发布")
            api = ctypes.WinDLL("version", use_last_error=True)
            api.GetFileVersionInfoSizeW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p]
            api.GetFileVersionInfoW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
            api.VerQueryValueW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint32)]
            size = api.GetFileVersionInfoSizeW(str(candidates[0]), None)
            if not size:
                raise SystemExit("安装包缺少版本信息")
            buffer = ctypes.create_string_buffer(size)
            if not api.GetFileVersionInfoW(str(candidates[0]), 0, size, buffer):
                raise SystemExit("无法读取安装包版本")
            pointer = ctypes.c_void_p(); length = ctypes.c_uint32()
            if not api.VerQueryValueW(buffer, "\\", ctypes.byref(pointer), ctypes.byref(length)):
                raise SystemExit("无法解析安装包版本")
            info = ctypes.cast(pointer, ctypes.POINTER(ctypes.c_uint32 * 13)).contents
            actual = f"{info[2] >> 16}.{info[2] & 65535}.{info[3] >> 16}"
            if actual != version:
                raise SystemExit(f"Windows 包版本 {actual} 与源码 {version} 不一致")
        destination = dist / name
        shutil.copyfile(candidates[0], destination)
        assets.append({"platform": platform, "cpu": cpu, "url": f"https://github.com/{REPO}/releases/download/v{version}/{name}",
                       "sha256": digest(destination), "size": destination.stat().st_size})
    notes = args.notes.read_text(encoding="utf-8")
    lines = [line[2:].strip() for line in notes.splitlines() if line.startswith("- ")]
    index = {"schema_version": 1, "releases": [{"version": version, "channel": "stable", "date": str(datetime.now(timezone(timedelta(hours=8))).date()), "notes": lines, "assets": assets}]}
    save(dist / "releases.json", index)
    signer = ROOT / "target/release/qingjian-release-sign.exe"
    run(signer, "sign", dist / "releases.json", "--key-file", private / "update-signing.dpapi")
    run(signer, "verify", dist / "releases.json")
    uploads = [dist / Path(asset["url"]).name for asset in assets] + [dist / "releases.json", dist / "releases.json.sig"]
    sums = dist / f"SHA256SUMS-v{version}.txt"
    sums.write_text("".join(f"{digest(path)}  {path.name}\n" for path in uploads), encoding="utf-8")
    uploads.append(sums)
    save(dist / "prepared.json", {"version": version, "commit": commit, "files": len(manifest), "uploads": {path.name: digest(path) for path in uploads}})
    print(f"Prepared {version}: {commit}, {len(manifest)} files", flush=True)
    if not args.publish:
        return
    prior_pending=json.loads(pending_path.read_text(encoding="utf-8")) if pending_path.exists() else None
    release=find_release(version)
    if release and not release["draft"]:
        raise SystemExit("该版本已经公开发布，禁止替换；请递增版本")
    save(pending_path,{"version":version,"commit":commit,"manifest":manifest})
    run("git", "push", "origin", "main", cwd=public)
    tag_probe=subprocess.run(["git","rev-parse",f"refs/tags/v{version}"],cwd=public,stdout=subprocess.PIPE,stderr=subprocess.PIPE,encoding="utf-8")
    if tag_probe.returncode==0 and tag_probe.stdout.strip()!=commit:
        if not (release and release["draft"] and prior_pending and prior_pending.get("commit")==tag_probe.stdout.strip()):
            raise SystemExit("已有标签不属于本次未发布草稿，禁止覆盖")
        # 仅恢复本工具刚创建、尚未公开的草稿；正式版本标签永不改写。
        run("git","tag","-d",f"v{version}",cwd=public)
        run("git","push","origin","--delete",f"v{version}",cwd=public)
        tag_probe=None
    if tag_probe is None or tag_probe.returncode!=0:
        run("git", "tag", f"v{version}", cwd=public)
    run("git", "push", "origin", f"v{version}", cwd=public)
    if release:
        run("gh","release","upload",f"v{version}","--repo",REPO,"--clobber",*uploads)
    else:
        run("gh", "release", "create", f"v{version}", "--repo", REPO, "--verify-tag", "--draft", "--title", f"猛人输入法 v{version} 正式版", "--notes-file", args.notes.resolve(), *uploads)
    release=find_release(version)
    if not release or not release["draft"]:
        raise SystemExit("未找到本次发布草稿，停止发布")
    uploaded = {asset["name"]: asset for asset in release["assets"]}
    if set(uploaded) != {path.name for path in uploads}:
        raise SystemExit("草稿发布附件列表不一致，未公开 Release")
    for path in uploads:
        if uploaded[path.name]["size"] != path.stat().st_size or uploaded[path.name].get("digest") != "sha256:" + digest(path):
            raise SystemExit("草稿附件校验不一致，未公开 Release")
    gh_json("--method","PATCH",f"repos/{REPO}/releases/{release['id']}","-F","draft=false","-f","make_latest=true")
    save(state_path, {"version": version, "commit": commit, "manifest": manifest})
    save(dist / "published.json", {"version": version, "commit": commit, "url": f"https://github.com/{REPO}/releases/tag/v{version}", "verified_uploads": len(uploads)})
    print(f"Published https://github.com/{REPO}/releases/tag/v{version}", flush=True)


if __name__ == "__main__":
    main()
