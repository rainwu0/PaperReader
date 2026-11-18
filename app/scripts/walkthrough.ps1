# 在 Android 模擬器上走「怎麼算成功」的七條主流程並截圖。細節在 `walkthrough/`。
#
# 用法（在任何位置）：
#   pwsh -NoProfile -File app\scripts\walkthrough.ps1 -Serial emulator-5580                 建置 → 安裝 → 七條主流程
#   pwsh -NoProfile -File app\scripts\walkthrough.ps1 -Serial emulator-5580 -SkipBuild      用已經建好的 APK
#   pwsh -NoProfile -File app\scripts\walkthrough.ps1 -Serial emulator-5580 -Only 5         只跑第 5 條（要先有跑過前面留下的文獻庫）
#   ... -OutDir 某個資料夾                                                             截圖與 results.json 放哪裡（預設 app\target\walkthrough，不進版本庫）
#   ... -Apk app\target\release-apk\PaperReader-0.1.0-x86_64.apk                           裝指定的安裝檔（例如正式版），不建置
#       正式版的網頁除錯介面是關著的；模擬器用 Google APIs 的系統映像（userdebug）時，系統對每個應用都打開它，
#       腳本才連得上。用 Play 商店版的映像（user）時連不上。
#
# -Serial 一定要給：同一台電腦上常同時開著幾台模擬器（各自操作的），腳本會清掉那一台上應用的資料。
# -DevtoolsPort：電腦這一端轉接網頁除錯用的連接埠，幾台同時跑時各用各的（預設依模擬器的編號算）。
#
# 前提：模擬器已開機（可以用 emulator.ps1 開）；JDK、Android SDK、NDK 的位置見 dev-env.ps1；有 Python（產生測試用的 PDF）。
# 權限（所有檔案存取）照使用者的路走：腳本在系統的授權頁按下開關。實機上由使用者自己按。
param(
    [Parameter(Mandatory)][string]$Serial,
    [switch]$SkipBuild,
    [ValidatePattern('^(all|[1-7])$')][string]$Only = 'all',
    [string]$OutDir = 'app\target\walkthrough',
    [int]$DevtoolsPort = 0,
    [string]$Apk = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$app = Join-Path $repo 'app'

. (Join-Path $PSScriptRoot 'dev-env.ps1') | Out-Null
# 這個專案的路徑是純英文，用預設的 app\target
$env:CARGO_TARGET_DIR = $null
$adb = Join-Path $env:ANDROID_HOME 'platform-tools\adb.exe'
$env:ADB = $adb

$state = (& $adb -s $Serial get-state 2>$null)
if ($state -ne 'device') {
    throw "找不到開機的模擬器 $Serial（adb devices 看得到哪幾台）"
}
$env:PR_SERIAL = $Serial
if ($DevtoolsPort -eq 0) {
    # emulator-5580 → 9580；不是模擬器的序號就用 9222
    $DevtoolsPort = if ($Serial -match 'emulator-(\d+)$') { 4000 + [int]$Matches[1] } else { 9222 }
}
$env:PR_DEVTOOLS_PORT = "$DevtoolsPort"

Set-Location $app
if ($Apk) {
    # 指定的安裝檔：相對路徑以版本庫為準
    $given = if ([IO.Path]::IsPathRooted($Apk)) { $Apk } else { Join-Path $repo $Apk }
    if (-not (Test-Path -LiteralPath $given)) { throw "找不到 APK：$given" }
    $env:PR_APK = (Resolve-Path -LiteralPath $given).Path
} else {
    $apkDir = Join-Path $app 'src-tauri\gen\android\app\build\outputs\apk\universal\debug'
    if (-not $SkipBuild) {
        Write-Host '=== 建置 APK（模擬器用，x86_64，除錯簽署）==='
        npx tauri android build --apk --debug --target x86_64
        if ($LASTEXITCODE -ne 0) { throw 'APK 建置失敗' }
    }
    # 不可以叫 $apk：PowerShell 的變數不分大小寫，會和字串型別的參數 $Apk 是同一個，檔案物件被轉成字串
    $found = Get-ChildItem $apkDir -Filter '*.apk' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $found) { throw "找不到 APK：$apkDir" }
    $env:PR_APK = $found.FullName
}

# 測試用的 PDF（合成的，放在 app\target\test-pdfs，不進版本庫）：沒有就產生
$samples = Join-Path $app 'target\test-pdfs'
if (-not (Test-Path (Join-Path $samples '合成-30頁.pdf'))) {
    # 在另一個 worktree 裡時，主工作資料夾已經產生過就直接用
    $common = (git -C $repo rev-parse --path-format=absolute --git-common-dir) 2>$null
    $mainSamples = if ($common) { Join-Path (Split-Path $common -Parent) 'app\target\test-pdfs' } else { $null }
    if ($mainSamples -and (Test-Path (Join-Path $mainSamples '合成-30頁.pdf'))) {
        $samples = $mainSamples
    } else {
        Write-Host '=== 產生測試用 PDF ==='
        python (Join-Path $PSScriptRoot 'make-test-pdfs.py') $samples
        if ($LASTEXITCODE -ne 0) { throw '產生測試用 PDF 失敗（要有 Python 3）' }
    }
}
$env:PR_SAMPLES = $samples
$env:PR_OUT = Join-Path $repo $OutDir

node (Join-Path $PSScriptRoot 'walkthrough\run.mjs') --only $Only
exit $LASTEXITCODE
