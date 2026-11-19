# 提交之前、以及日後持續整合要跑的同一組檢查。依序執行，任何一步失敗就停下並回報。
#
# 用法（在任何位置）：pwsh -NoProfile -File app\scripts\check.ps1
#
# 步驟：
#   1. npm run typecheck                                          介面的型別檢查（tsc --noEmit，嚴格模式）
#   2. npm run lint                                               介面的靜態檢查（Oxlint，設定在 app\.oxlintrc.json）
#   3. npm run test:ui                                            介面邏輯測試（搜尋、排序、篩選、PDF 自帶篇名的取捨；Node 內建測試器）
#      node scripts/version-check.mjs                             版號一致性：三處版號相同、格式合規、versionCode 照公式（規格\05）
#      node scripts/ext-boundary.mjs                             外掛的邊界：登記處以外沒有檔案引用外掛
#   4. node scripts/npm-audit.mjs                                 介面相依的已知漏洞（npm audit，要連網；audit-exceptions.json 列的例外放行）
#   5. node scripts/npm-licenses.mjs                              介面相依的授權，對照和 app\deny.toml 相同的允許清單
#   6. npm run build                                              介面建置。Rust 的應用外殼編譯時要內嵌建置結果，沒有它外殼編不過
#   7. cargo fmt --all --check                                    排版符合 rustfmt（設定在 app\rustfmt.toml）
#   8. cargo clippy --workspace --all-targets -- -D warnings      靜態檢查，警告即失敗
#   9. cargo deny check                                           相依的授權、已知漏洞、來源（設定在 app\deny.toml）
#  10. cargo test --workspace                                     全部測試
#
# Kotlin 的排版工具（ktfmt）需要另外下載安裝，所以 Kotlin 沒有自動排版檢查。
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)   # app\

# cargo-deny 不在 PATH 時，用開發工具資料夾（環境變數 PAPERREADER_TOOLS）裡解開的那一份
# （只在這個程序內暫時加進 PATH，不改系統或使用者的環境變數）。PAPERREADER_TOOLS 沒設就不找。
if (-not (Get-Command cargo-deny -ErrorAction SilentlyContinue) -and $env:PAPERREADER_TOOLS) {
    $denyDir = Join-Path $env:PAPERREADER_TOOLS 'cargo-deny-0.20.2\cargo-deny-0.20.2-x86_64-pc-windows-msvc'
    if (Test-Path (Join-Path $denyDir 'cargo-deny.exe')) { $env:Path = "$denyDir;$env:Path" }
}

# 介面的相依還沒裝（第一次、或剛換電腦）：依 package-lock.json 安裝，版本與鎖定的一致
if (-not (Test-Path 'node_modules')) {
    Write-Host '=== 第一次：npm ci（依 package-lock.json 安裝介面的相依）==='
    npm ci
    if ($LASTEXITCODE -ne 0) { Write-Host 'npm ci 失敗'; exit 1 }
}

$steps = @(
    # 選用：有 scripts/check-extra.ps1 時先跑它，放各自環境另加的檢查
    @{ Name = '額外的檢查';     Cmd = 'if (Test-Path scripts/check-extra.ps1) { pwsh -NoProfile -File scripts/check-extra.ps1 } else { Write-Host "略過：沒有 scripts/check-extra.ps1" }' },
    @{ Name = '介面型別檢查';   Cmd = 'npm run typecheck' },
    @{ Name = '介面靜態檢查';   Cmd = 'npm run lint' },
    @{ Name = '介面邏輯測試';   Cmd = 'npm run test:ui' },
    @{ Name = '外掛的邊界';     Cmd = 'node scripts/ext-boundary.mjs' },
    # tools/ 只在私人版本庫（不進公開快照），沒有就略過
    @{ Name = '專案工具的測試'; Cmd = 'if (Test-Path ../tools/tests) { py -3 -m unittest discover -s ../tools/tests } else { Write-Host "略過：沒有 tools/" }' },
    @{ Name = '版號一致性';     Cmd = 'node scripts/version-check.mjs' },
    @{ Name = '介面相依的漏洞'; Cmd = 'node scripts/npm-audit.mjs' },
    @{ Name = '介面相依的授權'; Cmd = 'node scripts/npm-licenses.mjs' },
    @{ Name = '介面建置';       Cmd = 'npm run build' },
    @{ Name = '排版';           Cmd = 'cargo fmt --all --check' },
    @{ Name = '靜態檢查';       Cmd = 'cargo clippy --workspace --all-targets -- -D warnings' },
    @{ Name = '相依與授權';     Cmd = 'cargo deny check' },
    @{ Name = '測試';           Cmd = 'cargo test --workspace' }
)

$n = 0
foreach ($s in $steps) {
    $n++
    Write-Host ''
    Write-Host "=== 第 $n 步（共 $($steps.Count) 步）：$($s.Name)：$($s.Cmd) ==="
    Invoke-Expression $s.Cmd
    if ($LASTEXITCODE -ne 0) {
        Write-Host ''
        Write-Host "失敗：第 $n 步（$($s.Name)）結束碼 $LASTEXITCODE。後面的步驟沒有執行。"
        if ($s.Cmd -like 'cargo deny*') {
            Write-Host '提示：若訊息是「no such command: deny」，表示還沒安裝 cargo-deny（cargo install cargo-deny --locked）。'
        }
        exit 1
    }
}

Write-Host ''
Write-Host "全部 $($steps.Count) 步通過。"
