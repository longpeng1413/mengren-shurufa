<#
.SYNOPSIS
    取本机离线语音识别引擎（sherpa-onnx C API Windows x64）到 target\installer\sherpa\。
.DESCRIPTION
    版本必须与 crates\qingjian-voice-offline\src\library\mod.rs 的 EXPECTED_VERSION 一致，换版本要一起改。
    包取自 pub.dev 的 sherpa_onnx_windows（与安卓端 sherpa_onnx_android_arm64 同源），按 SHA-256 核对，已取过就跳过。
    里面自带的 onnxruntime.dll 与 C API 是同一版编译的，必须成套装到 Server / 设置程序同目录：
    混用 NuGet 单独抓的 ONNX Runtime 会加载失败。
#>
$ErrorActionPreference = 'Stop'
# 进度条会让 Invoke-WebRequest 慢好几倍
$ProgressPreference = 'SilentlyContinue'

$Version = '2.0.1'
$Sha256 = '449b6ccb864713d454b771ebfe200d6e030e72cd1da9ae5bd1f079064550e435'
# 换了包（版本或架构）就改这个，旧的缓存才会重取
$StampValue = "$Version-c-api-x64"
$ArchiveName = "sherpa_onnx_windows-$Version.tar.gz"
$Files = @(
    'sherpa-onnx-c-api.dll',
    'onnxruntime.dll',
    'onnxruntime_providers_shared.dll',
    'sherpa-onnx-LICENSE.txt'
)

$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$Dest = Join-Path $Repo 'target\installer\sherpa'
$Stamp = Join-Path $Dest 'VERSION'
$absent = @($Files | Where-Object { -not (Test-Path (Join-Path $Dest $_)) })
if ((Test-Path $Stamp) -and ((Get-Content $Stamp -Raw).Trim() -eq $StampValue) -and ($absent.Count -eq 0)) {
    Write-Host "sherpa-onnx $Version Windows x64 已在 $Dest" -ForegroundColor Cyan
    return
}

$tmp = Join-Path $env:TEMP "sherpa-onnx-windows-$Version-setup"
if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
New-Item -ItemType Directory -Path $tmp -Force | Out-Null
$archive = Join-Path $tmp $ArchiveName
$mirrors = @(
    "https://pub.flutter-io.cn/api/archives/$ArchiveName",
    "https://pub.dev/api/archives/$ArchiveName"
)
$ok = $false
foreach ($url in $mirrors) {
    try {
        Write-Host "下载 $url" -ForegroundColor Cyan
        Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing -TimeoutSec 300
        $hash = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLower()
        if ($hash -ne $Sha256) {
            Write-Host "SHA-256 是 $hash，不是 $Sha256，换下一个镜像" -ForegroundColor Yellow
            Remove-Item $archive -Force
            continue
        }
        $ok = $true
        break
    } catch {
        Write-Host "取包失败：$($_.Exception.Message)" -ForegroundColor Yellow
    }
}
if (-not $ok) { throw "下载 sherpa-onnx Windows 库失败或 SHA-256 校验不通过：$ArchiveName" }

$stage = Join-Path $tmp 'extract'
New-Item -ItemType Directory -Path $stage -Force | Out-Null
& tar -xzf $archive -C $stage
if ($LASTEXITCODE -ne 0) { throw "解开 $ArchiveName 失败（退出码 $LASTEXITCODE）" }

$x64 = Join-Path $stage 'windows\x64'
foreach ($name in @('sherpa-onnx-c-api.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll')) {
    $src = Join-Path $x64 $name
    if (-not (Test-Path $src)) { throw "$ArchiveName 里没有 windows/x64/$name" }
}
$license = Join-Path $stage 'LICENSE'
if (-not (Test-Path $license)) { throw "$ArchiveName 里没有 LICENSE" }

if (Test-Path $Dest) { Remove-Item $Dest -Recurse -Force }
New-Item -ItemType Directory -Path $Dest -Force | Out-Null
foreach ($name in @('sherpa-onnx-c-api.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll')) {
    Copy-Item (Join-Path $x64 $name) -Destination (Join-Path $Dest $name) -Force
}
Copy-Item $license -Destination (Join-Path $Dest 'sherpa-onnx-LICENSE.txt') -Force
Set-Content -Path $Stamp -Value $StampValue -Encoding ASCII
Remove-Item $tmp -Recurse -Force
Write-Host "sherpa-onnx $Version Windows x64 → $Dest" -ForegroundColor Green
