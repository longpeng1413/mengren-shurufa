<#
.SYNOPSIS
    猛人输入法：一条命令编译安卓 APK 和 / 或 Windows 安装包，成品拷到 mengren\安装包\（不进仓库）。

.DESCRIPTION
    把本机编译要的特殊设置都包在这里（原因见 mengren\开发笔记.md）：
      - 从纯英文路径 D:\mengren-dev\shurufa 进入（安卓构建工具不接受中文项目路径）；
      - 安卓的 Rust 中间文件放 D:\mengren-dev\target（NDK 链接器打不开中文真实路径）；
      - 下载走本机 Clash 代理 127.0.0.1:7897；
      - 安卓关掉 Kotlin 增量编译（中英两条路径会让增量缓存记混，报「找不到」官方函数）；
      - Windows 不签名打包（官方 build.ps1 不加 -Sign 时自动 QINGJIAN_UIACCESS=0）。

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File D:\mengren-dev\shurufa\mengren\build.ps1            # 两个都编
    powershell -ExecutionPolicy Bypass -File D:\mengren-dev\shurufa\mengren\build.ps1 -Android   # 只编安卓
    powershell -ExecutionPolicy Bypass -File D:\mengren-dev\shurufa\mengren\build.ps1 -Windows   # 只编 Windows
#>
param([switch]$Android, [switch]$Windows)
$ErrorActionPreference = 'Stop'
if (-not $Android -and -not $Windows) { $Android = $true; $Windows = $true }

$Repo = 'D:\mengren-dev\shurufa'
$Out = Join-Path $Repo 'mengren\安装包'
$Proxy = 'http://127.0.0.1:7897'
New-Item -ItemType Directory -Force $Out | Out-Null
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:LOCALAPPDATA\Programs\Inno Setup 7;" + $env:Path
$env:HTTPS_PROXY = $Proxy
$env:HTTP_PROXY = $Proxy

# 新包校验通过后，每个平台只保留最新包，旧包送入垃圾箱。
function Prune([string]$Pattern, [string]$Keep) {
    Add-Type -AssemblyName Microsoft.VisualBasic
    $outputPath = [IO.Path]::GetFullPath($Out).TrimEnd('\')
    foreach ($package in Get-ChildItem -LiteralPath $Out -File -Filter $Pattern) {
        if ([IO.Path]::GetFullPath($package.DirectoryName).TrimEnd('\') -ne $outputPath) {
            throw "安装包路径越界：$($package.FullName)"
        }
        if ($package.FullName -eq $Keep) { continue }
        if ($package.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "不处理链接文件：$($package.FullName)" }
        [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile(
            $package.FullName,
            [Microsoft.VisualBasic.FileIO.UIOption]::OnlyErrorDialogs,
            [Microsoft.VisualBasic.FileIO.RecycleOption]::SendToRecycleBin)
        Write-Host "已移到垃圾箱：$($package.Name)"
    }
}

function Verify-Package([string]$Source, [string]$Destination) {
    if ((Get-FileHash -LiteralPath $Source).Hash -ne (Get-FileHash -LiteralPath $Destination).Hash) {
        throw "安装包复制校验失败：$Destination"
    }
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmm'

if ($Android) {
    $env:CARGO_TARGET_DIR = 'D:\mengren-dev\target'
    Push-Location (Join-Path $Repo 'apps\android')
    try {
        & .\gradlew.bat assembleRelease --console=plain '-Pkotlin.incremental=false' `
            '-Dhttp.proxyHost=127.0.0.1' '-Dhttp.proxyPort=7897' '-Dhttps.proxyHost=127.0.0.1' '-Dhttps.proxyPort=7897'
        if ($LASTEXITCODE -ne 0) { throw "安卓编译失败（退出码 $LASTEXITCODE）" }
    } finally {
        # Gradle / Kotlin 常驻进程会占着调用方的输出管道，后台调用本脚本时会一直等不到结束；编完就停掉
        & .\gradlew.bat --stop | Out-Null
        Get-CimInstance Win32_Process -Filter "Name='java.exe'" |
            Where-Object { $_.CommandLine -match 'KotlinCompileDaemon' } |
            ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
        Pop-Location
    }
    Remove-Item Env:CARGO_TARGET_DIR
    $apk = Join-Path $Out "猛人输入法-安卓-$stamp.apk"
    Copy-Item (Join-Path $Repo 'apps\android\app\build\outputs\apk\release\app-release.apk') $apk
    Verify-Package (Join-Path $Repo 'apps\android\app\build\outputs\apk\release\app-release.apk') $apk
    Prune '猛人输入法-安卓-*.apk' $apk
    Write-Host "安卓：$apk" -ForegroundColor Green
}

if ($Windows) {
    Push-Location $Repo
    try {
        powershell -NoProfile -ExecutionPolicy Bypass -File apps\windows\installer\build.ps1
        if ($LASTEXITCODE -ne 0) { throw "Windows 打包失败（退出码 $LASTEXITCODE）" }
    } finally { Pop-Location }
    $setup = Get-ChildItem (Join-Path $Repo 'target\installer') -Filter '*-setup.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $dest = Join-Path $Out "猛人输入法-Windows-$stamp-setup.exe"
    Copy-Item $setup.FullName $dest
    Verify-Package $setup.FullName $dest
    Prune '猛人输入法-Windows-*-setup.exe' $dest
    Write-Host "Windows：$dest" -ForegroundColor Green
}
