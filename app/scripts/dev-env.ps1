# 設定 Android 建置要用的環境變數：JAVA_HOME、ANDROID_HOME（ANDROID_SDK_ROOT）、NDK_HOME，並把 JDK 與 adb 加進 PATH。
# 只影響目前這個 PowerShell 視窗，不改系統或使用者的設定。
#
# 用法（點號載入，在任何位置）：. app\scripts\dev-env.ps1
#
# 位置怎麼決定：
#   1. JAVA_HOME、ANDROID_HOME、NDK_HOME 都已經設了：照用。
#   2. 否則用 PAPERREADER_TOOLS（開發工具都裝在同一個資料夾時）：
#        JAVA_HOME    = <PAPERREADER_TOOLS>\jdk17
#        ANDROID_HOME = <PAPERREADER_TOOLS>\Android\Sdk
#        NDK_HOME     = <ANDROID_HOME>\ndk\28.2.13676358
#      已經設了的那幾個不覆蓋。
#   3. 兩者都沒有：停下並說明要設什麼。
# 要裝哪些工具見 README.md 的「從原始碼建置」。
$ndkVersion = '28.2.13676358'
if (-not ($env:JAVA_HOME -and $env:ANDROID_HOME -and $env:NDK_HOME)) {
    $tools = $env:PAPERREADER_TOOLS
    if (-not $tools) {
        throw ('找不到 Android 建置工具：請設 JAVA_HOME、ANDROID_HOME、NDK_HOME，' +
            '或設 PAPERREADER_TOOLS 指向放 jdk17 與 Android\Sdk 的資料夾（例如 $env:PAPERREADER_TOOLS = ''C:\dev-tools''）。')
    }
    if (-not $env:JAVA_HOME) { $env:JAVA_HOME = Join-Path $tools 'jdk17' }
    if (-not $env:ANDROID_HOME) { $env:ANDROID_HOME = Join-Path $tools 'Android\Sdk' }
    if (-not $env:NDK_HOME) { $env:NDK_HOME = Join-Path $env:ANDROID_HOME "ndk\$ndkVersion" }
}
foreach ($v in 'JAVA_HOME', 'ANDROID_HOME', 'NDK_HOME') {
    $p = [Environment]::GetEnvironmentVariable($v)
    if (-not (Test-Path -LiteralPath $p)) { throw "$v 指到的資料夾不存在：$p" }
}
$env:ANDROID_SDK_ROOT = $env:ANDROID_HOME
$env:Path = "$env:JAVA_HOME\bin;$env:ANDROID_HOME\platform-tools;$env:Path"
Write-Host "JAVA_HOME=$env:JAVA_HOME"
Write-Host "ANDROID_HOME=$env:ANDROID_HOME"
Write-Host "NDK_HOME=$env:NDK_HOME"
