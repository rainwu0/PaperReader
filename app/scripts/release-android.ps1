# 建正式版的 Android 安裝檔：每個架構一個（arm64 給大部分手機，x86_64 給模擬器與少數裝置），用你自己的金鑰簽章，
# 核對簽章，印出大小與 SHA-256，複製到 app\target\release-apk\（連同 SHA256SUMS.txt，可以直接放上 GitHub Releases）。
#
# 用法（在任何位置）：
#   pwsh -NoProfile -File app\scripts\release-android.ps1                      兩個架構都建
#   pwsh -NoProfile -File app\scripts\release-android.ps1 -Targets aarch64     只建 arm64
#   在模擬器上驗：walkthrough.ps1 -Serial <模擬器> -Apk app\target\release-apk\PaperReader-<版本>-x86_64.apk
#
# 安裝檔名：PaperReader-<版號>-<架構>.apk，例 PaperReader-0.1.1-arm64.apk；修正版是 PaperReader-0.1.1.1-arm64.apk（對外的版號用點，不出現 +；規格\05 第 1、7 節）。
# 版號用 app\scripts\bump-version.mjs 改；發布前這個腳本會先跑 version-check.mjs。
#
# 前提：
#   - app\src-tauri\gen\android\keystore.properties 寫好金鑰檔的位置、別名與密碼（欄位見同一層的
#     keystore.properties.example）。這個檔被 .gitignore 擋；缺了腳本會說明要做什麼再停下。
#   - JDK、Android SDK（含 build-tools 的 apksigner）、NDK；位置照 dev-env.ps1 決定（JAVA_HOME 等環境變數，或 PAPERREADER_TOOLS）。
#   - 編譯輸出沿用目前的 CARGO_TARGET_DIR（沒設就是 app\target）。
#
# 腳本不印密碼，也不印金鑰的內容；印的是憑證的主體名稱與 SHA-256 指紋（公開的資訊，安裝檔裡本來就有）。
param(
    [ValidateSet('aarch64', 'x86_64')][string[]]$Targets = @('aarch64', 'x86_64')
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$app = Join-Path $repo 'app'
$android = Join-Path $app 'src-tauri\gen\android'
$props = Join-Path $android 'keystore.properties'

# ---------- 0. 版號 ----------
# 發布前先核對版號（規格\05_版號規則.md）：格式、三處一致、versionCode 照公式。不過就停。
node (Join-Path $PSScriptRoot 'version-check.mjs')
if ($LASTEXITCODE -ne 0) { Write-Host '版號檢查沒過，不建置。'; exit 1 }
$conf = Get-Content (Join-Path $app 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
# 對外的版號：設定檔的 a.b.c+d 換成 a.b.c.d（檔名、標籤、說明都用這個）
$publicVersion = $conf.version.Replace('+', '.')
Write-Host "版號 $publicVersion（設定檔 $($conf.version)），versionCode $($conf.bundle.android.versionCode)"
Write-Host "安裝檔名：PaperReader-$publicVersion-<架構>.apk"

# ---------- 1. 簽章設定 ----------
if (-not (Test-Path $props)) {
    Write-Host ''
    Write-Host "找不到簽章設定：$props"
    Write-Host ''
    Write-Host '要先做的事（只做一次）：'
    Write-Host '  1. 產生簽章金鑰（用 keytool 產生，自己備份），金鑰檔不要放在版本庫裡。'
    Write-Host '  2. 把 keystore.properties.example 複製成 keystore.properties（同一個資料夾）：'
    Write-Host "       Copy-Item '$android\keystore.properties.example' '$props'"
    Write-Host '  3. 用記事本打開 keystore.properties，填 password、keyAlias、storeFile 三欄。'
    Write-Host '  4. 再執行一次這個腳本。'
    exit 1
}
# 只讀 storeFile 確認金鑰檔在；密碼留給 Gradle 讀
$fields = @{}
foreach ($line in Get-Content -LiteralPath $props -Encoding utf8) {
    if ($line -match '^\s*([A-Za-z]+)\s*[=:]\s*(.*?)\s*$') { $fields[$Matches[1]] = $Matches[2] }
}
foreach ($k in 'password', 'keyAlias', 'storeFile') {
    if (-not $fields[$k]) { Write-Host "keystore.properties 缺少 $k（欄位見 keystore.properties.example）"; exit 1 }
}
$storeFile = $fields['storeFile'].Replace('\\', '\')
if (-not (Test-Path -LiteralPath $storeFile)) {
    Write-Host "keystore.properties 的 storeFile 指到的金鑰檔不存在：$storeFile"
    exit 1
}

# ---------- 2. 環境 ----------
. (Join-Path $PSScriptRoot 'dev-env.ps1') | Out-Null
$buildTools = Get-ChildItem (Join-Path $env:ANDROID_HOME 'build-tools') -Directory |
    Sort-Object { [version]($_.Name -replace '[^0-9.].*$', '') } | Select-Object -Last 1
$apksigner = Join-Path $buildTools.FullName 'apksigner.bat'
if (-not (Test-Path $apksigner)) { throw "找不到 apksigner：$apksigner" }

$version = $publicVersion   # 對外的版號（上面算好的）
$abiOf = @{ aarch64 = 'arm64'; x86_64 = 'x86_64' }

# ---------- 3. 建置 ----------
Set-Location $app
# 先清掉上一次的安裝檔：Gradle 認為沒變時不會重寫，留著舊檔會分不出是不是這次建的
foreach ($t in $Targets) {
    $old = Join-Path $android "app\build\outputs\apk\$($abiOf[$t])\release"
    if (Test-Path $old) { Remove-Item -Recurse -Force $old }
}
$buildArgs = @('tauri', 'android', 'build', '--apk', '--split-per-abi', '--ci', '--target') + $Targets
Write-Host "=== 建置正式版 $version：$($Targets -join '、') ==="
& npx @buildArgs
if ($LASTEXITCODE -ne 0) { Write-Host '建置失敗（輸出在上面）'; exit 1 }

# ---------- 4. 核對簽章、複製、算雜湊 ----------
$outDir = Join-Path $app 'target\release-apk'
New-Item -ItemType Directory -Force $outDir | Out-Null
$sums = @()
$failed = $false
foreach ($t in $Targets) {
    $abi = $abiOf[$t]
    $dir = Join-Path $android "app\build\outputs\apk\$abi\release"
    $apk = Get-ChildItem $dir -Filter '*.apk' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $apk) { Write-Host "找不到這次建出的安裝檔：$dir"; $failed = $true; continue }
    if ($apk.Name -like '*unsigned*') { Write-Host "安裝檔沒有簽章：$($apk.FullName)（檢查 keystore.properties）"; $failed = $true; continue }

    Write-Host ''
    Write-Host "=== 核對簽章：$abi ==="
    $verify = & $apksigner verify --verbose --print-certs $apk.FullName 2>&1
    if ($LASTEXITCODE -ne 0) {
        $verify | ForEach-Object { Write-Host "  $_" }
        Write-Host "簽章核對失敗：$($apk.FullName)"
        $failed = $true
        continue
    }
    $verify | Where-Object { $_ -match '^Verifie[sd]|^Verified using|certificate DN|certificate SHA-256 digest|^Number of signers' } |
        ForEach-Object { Write-Host "  $_" }

    $name = "PaperReader-$version-$abi.apk"
    $dest = Join-Path $outDir $name
    Copy-Item $apk.FullName $dest -Force
    $hash = (Get-FileHash $dest -Algorithm SHA256).Hash.ToLowerInvariant()
    $size = (Get-Item $dest).Length
    $sums += "$hash  $name"
    Write-Host ("  {0}：{1:N0} 位元組（{2:N1} MB）" -f $name, $size, ($size / 1MB))
    Write-Host "  SHA-256：$hash"
}
if ($sums.Count -gt 0) {
    $sumFile = Join-Path $outDir 'SHA256SUMS.txt'
    [IO.File]::WriteAllText($sumFile, (($sums -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
    Write-Host ''
    Write-Host "安裝檔與 SHA256SUMS.txt 在 $outDir"
}
if ($failed) { exit 1 }
