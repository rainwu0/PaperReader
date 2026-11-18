# 建立並啟動一台 Android 模擬器（電腦上跑的虛擬手機），等它開機完成。
#
# 用法（在任何位置）：
#   pwsh -NoProfile -File app\scripts\emulator.ps1                        名為 pilot 的那台，用模擬器自己挑的連接埠
#   pwsh -NoProfile -File app\scripts\emulator.ps1 -Name snap -Port 5590   名為 snap 的那台，序號是 emulator-5590
#
# 前提：環境見 dev-env.ps1；已用 sdkmanager 裝好 emulator 與 system-images;android-36;google_apis;x86_64。
# 虛擬手機放在 ANDROID_AVD_HOME；沒設時放在 <PAPERREADER_TOOLS>\avd；兩者都沒設時用 Android 預設的位置。
# 沒有這台就先建（Pixel 6 的外形、4 GB 記憶體、8 GB 資料區）。不改系統設定與使用者的環境變數。
param(
    [string]$Name = 'pilot',
    [int]$Port = 0
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'dev-env.ps1') | Out-Null
if (-not $env:ANDROID_AVD_HOME -and $env:PAPERREADER_TOOLS) {
    $env:ANDROID_AVD_HOME = Join-Path $env:PAPERREADER_TOOLS 'avd'
}
$avdHome = if ($env:ANDROID_AVD_HOME) { $env:ANDROID_AVD_HOME } else { Join-Path $HOME '.android\avd' }
$adb = Join-Path $env:ANDROID_HOME 'platform-tools\adb.exe'

$config = Join-Path $avdHome "$Name.avd\config.ini"
if (-not (Test-Path -LiteralPath $config)) {
    New-Item -ItemType Directory -Force $avdHome | Out-Null
    # avdmanager 會抱怨讀不到 devices.xml，但仍會建出設定檔，後面再補齊
    $avdmanager = Join-Path $env:ANDROID_HOME 'cmdline-tools\latest\bin\avdmanager.bat'
    "no" | & $avdmanager create avd -n $Name `
        -k "system-images;android-36;google_apis;x86_64" -d pixel_6 --force | Out-Null
    $t = (Get-Content $config) `
        -replace '^hw.ramSize=.*', 'hw.ramSize=4096' `
        -replace '^hw.gpu.enabled=.*', 'hw.gpu.enabled=yes' `
        -replace '^avd.name=.*', "avd.name=$Name" `
        -replace '^avd.id=.*', "avd.id=$Name" `
        -replace '^hw.cpu.ncore=.*', 'hw.cpu.ncore=6' `
        -replace '^disk.dataPartition.size=.*', 'disk.dataPartition.size=8G'
    Set-Content $config $t -Encoding ascii
}

$emuArgs = @('-avd', $Name, '-no-snapshot', '-no-audio', '-no-boot-anim', '-gpu', 'auto')
if ($Port) { $emuArgs += @('-port', "$Port") }
Start-Process -FilePath (Join-Path $env:ANDROID_HOME 'emulator\emulator.exe') -ArgumentList $emuArgs `
    -RedirectStandardOutput (Join-Path $avdHome "$Name.log") -RedirectStandardError (Join-Path $avdHome "$Name.err")
$target = if ($Port) { @('-s', "emulator-$Port") } else { @() }
& $adb @target wait-for-device
do { Start-Sleep 5; $b = (& $adb @target shell getprop sys.boot_completed 2>$null) } until ($b -match "1")
Write-Host "模擬器 $Name 已開機。"
& $adb devices -l
