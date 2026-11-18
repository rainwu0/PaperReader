# 在 Android 模擬器上驗證文獻庫核心。
#
# 做的事：編成 Android 執行檔 prcli → 推到模擬器 → 依序跑十個項目，每項印出「通過」或「失敗」與實際看到的內容。
# 用法（在 app 底下）：pwsh -NoProfile -File scripts\emulator-check.ps1 [-SkipBuild]
# 全程記錄寫到 target\emulator-check.log（版本庫忽略 target\）。
# 開著幾台模擬器時，先設 $env:ANDROID_SERIAL = 'emulator-NNNN' 指定哪一台（adb 自己認這個變數）。
#
# 前提：JDK、Android SDK、NDK 的位置見 dev-env.ps1；有 Python（產生測試用的 PDF）。
# 對環境的影響：只在目前這個 PowerShell 視窗設定環境變數；沒有開著的模擬器時用 emulator.ps1 開機。
# 在模擬器上只動 /data/local/tmp/prcli* 與 /sdcard/Documents/PaperReaderSpec1/，不碰 Documents/PaperReader/。
# 項目 13 到 19 是全局功能的項目：群組、譯本、筆記檔、備份與還原；它們另外會寫
# /sdcard/Documents/pr-backup*.zip 與 PaperReaderRestore*，收尾時刪掉。
param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
$appDir  = Split-Path $PSScriptRoot -Parent
$projDir = Split-Path $appDir -Parent
. (Join-Path $PSScriptRoot 'dev-env.ps1') | Out-Null
$env:CARGO_TARGET_DIR = Join-Path $appDir 'target'
$env:CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER = Join-Path $env:NDK_HOME 'toolchains\llvm\prebuilt\windows-x86_64\bin\x86_64-linux-android24-clang.cmd'
$adb = Join-Path $env:ANDROID_HOME 'platform-tools\adb.exe'

Start-Transcript -Path (Join-Path $appDir 'target\emulator-check.log') -Force | Out-Null

$P    = '/data/local/tmp/prcli'
$DEV  = '/data/local/tmp/prcli-device'
$SRC  = '/data/local/tmp/prcli-src'
$ROOT = '/sdcard/Documents/PaperReaderSpec1'

# ---------- 小工具 ----------
$script:LastExit = 0
function Sh([string]$cmd) {
    $o = & $adb shell $cmd 2>&1
    $script:LastExit = $LASTEXITCODE
    (($o | ForEach-Object { "$_" }) -join "`n").TrimEnd()
}
function Pr([string]$argline) {
    $txt = Sh "$P $ROOT $DEV $argline"
    try { return , (ConvertFrom-Json -InputObject $txt -DateKind String -NoEnumerate) }
    catch { throw "prcli 的輸出不是 JSON：$txt" }
}
function WaitBoot {
    & $adb wait-for-device | Out-Null
    for ($i = 0; $i -lt 90; $i++) {
        try {
            $b = (& $adb shell getprop sys.boot_completed 2>$null) -join ''
            if ($b.Trim() -eq '1') {
                $ok = (& $adb shell "ls /sdcard/Android >/dev/null 2>&1 && echo ready" 2>$null) -join ''
                if ($ok.Trim() -eq 'ready') { return }
            }
        } catch { }
        Start-Sleep 3
    }
    throw '開機等候逾時'
}

$results = [System.Collections.Generic.List[object]]::new()
function Item([int]$n, [string]$name, [scriptblock]$body) {
    Write-Host ''
    Write-Host "=== 項目 ${n}：$name ==="
    $sw = [Diagnostics.Stopwatch]::StartNew()
    try {
        $r = & $body
        $ok = [bool]$r.Pass
        $detail = ($r.Detail -join "`n")
    } catch {
        $ok = $false
        $detail = "例外：$($_.Exception.Message)"
    }
    $sw.Stop()
    $word = if ($ok) { '通過' } else { '失敗' }
    Write-Host $detail
    # 每項做完記一次 /sdcard 的剩餘空間（只記錄）：大檔的項目多，空間不夠時錯誤會出現在別的地方
    Write-Host ("剩餘空間：" + (Sh "df -h /sdcard | tail -1"))
    Write-Host ("[{0}] 項目 {1}：{2}（本項耗時 {3:N1} 秒，只記錄）" -f $word, $n, $name, $sw.Elapsed.TotalSeconds)
    $results.Add([pscustomobject]@{ N = $n; Name = $name; Pass = $ok; Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1) })
}

# ---------- 準備：編譯、開機、推送 ----------
if (-not $SkipBuild) {
    Write-Host '--- 編譯 prcli（x86_64-linux-android, release）'
    Push-Location $appDir
    try {
        cargo build --release --target x86_64-linux-android -p prcli
        if ($LASTEXITCODE -ne 0) { throw 'cargo build 失敗' }
    } finally { Pop-Location }
}
$bin = Join-Path $appDir 'target\x86_64-linux-android\release\prcli'

$devices = (& $adb devices) -join "`n"
if ($devices -notmatch 'emulator-\d+\s+device') {
    Write-Host '--- 開模擬器'
    & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'emulator.ps1')
}
WaitBoot

Write-Host '--- 推送執行檔與測試檔'
Sh "rm -rf $ROOT $SRC $DEV /data/local/tmp/prcli-bg.out; mkdir -p $SRC" | Out-Null
& $adb push $bin $P | Out-Null
Sh "chmod 755 $P" | Out-Null
# 三份合成 PDF：依大小對應（約 0.35MB、5.3MB、53MB），推到英文檔名，避開 adb 在 Windows 上的中文檔名問題
# 測試檔不進版本庫，放在 app\target\test-pdfs：在別的 worktree 裡時，主工作資料夾已經產生過就用那一份；都沒有就產生
$samples = Join-Path $appDir 'target\test-pdfs'
if (-not (Test-Path (Join-Path $samples '合成-30頁.pdf'))) {
    $common = (git -C $projDir rev-parse --path-format=absolute --git-common-dir) 2>$null
    $mainSamples = if ($common) { Join-Path (Split-Path $common -Parent) 'app\target\test-pdfs' } else { $null }
    if ($mainSamples -and (Test-Path (Join-Path $mainSamples '合成-30頁.pdf'))) {
        $samples = $mainSamples
    } else {
        python (Join-Path $PSScriptRoot 'make-test-pdfs.py') $samples
        if ($LASTEXITCODE -ne 0) { throw '產生測試用 PDF 失敗（要有 Python 3）' }
    }
}
$srcPdfs = Get-ChildItem $samples -Filter *.pdf | Sort-Object Length
if ($srcPdfs.Count -ne 3) { throw "$samples 應有 3 份 PDF，實際 $($srcPdfs.Count)" }
$pdfNames = @('s30.pdf', 's5.pdf', 'big53.pdf')
$pdfSize = @{}
for ($i = 0; $i -lt 3; $i++) {
    & $adb push $srcPdfs[$i].FullName "$SRC/$($pdfNames[$i])" | Out-Null
    $pdfSize[$pdfNames[$i]] = $srcPdfs[$i].Length
}
Write-Host '--- 在裝置上產生 400MB（中斷測試）、53MB（計時）與三份小檔（大小寫測試、重新掃描測試）的隨機內容'
# 核心加入時驗檔頭，所以這些檔的開頭要換成 %PDF-（只覆寫前 5 個位元組，大小不變）
Sh "dd if=/dev/urandom of=$SRC/prcli-400m.bin bs=1M count=400 2>&1 | tail -1; printf '%%PDF-' | dd of=$SRC/prcli-400m.bin conv=notrunc 2>&1 | tail -1" | Out-Null
Sh "dd if=/dev/urandom of=$SRC/prcli-53m.bin bs=1M count=53 2>&1 | tail -1; printf '%%PDF-' | dd of=$SRC/prcli-53m.bin conv=notrunc 2>&1 | tail -1" | Out-Null
Sh "dd if=/dev/urandom of=$SRC/case1.bin bs=1k count=64 2>&1 | tail -1; printf '%%PDF-' | dd of=$SRC/case1.bin conv=notrunc 2>&1 | tail -1" | Out-Null
Sh "dd if=/dev/urandom of=$SRC/case2.bin bs=1k count=64 2>&1 | tail -1; printf '%%PDF-' | dd of=$SRC/case2.bin conv=notrunc 2>&1 | tail -1" | Out-Null
Sh "dd if=/dev/urandom of=$SRC/rescan-new.bin bs=1k count=64 2>&1 | tail -1; printf '%%PDF-' | dd of=$SRC/rescan-new.bin conv=notrunc 2>&1 | tail -1" | Out-Null

$ids = @{}      # 項目之間共用：key → 文獻代號
$folders = @{}  # key → 目前的資料夾名稱

# ---------- 項目 1 ----------
Item 1 'init，加入三份合成 PDF，列出資料夾並取出 paper.json' {
    $d = @()
    $init = Pr 'init'
    $d += "init → $($init | ConvertTo-Json -Compress)"
    $specs = @(
        @{ Key = 'alpha'; File = 's30.pdf';   Args = "--title 'Synthetic Thirty' --family Alpha --year 2024 --pages 30";  Folder = 'Alpha 2024 - Synthetic Thirty' },
        @{ Key = 'team';  File = 's5.pdf';    Args = "--title 'Synthetic Five' --literal 'Team B'";                         Folder = 'Team B - Synthetic Five' },
        @{ Key = 'gamma'; File = 'big53.pdf'; Args = "--title 'Synthetic Large' --family Gamma --year 2023 --pages 620";    Folder = 'Gamma 2023 - Synthetic Large' }
    )
    $pass = $true
    foreach ($s in $specs) {
        $r = Pr "add $SRC/$($s.File) $($s.Args)"
        $d += "add $($s.File) → $($r | ConvertTo-Json -Compress)"
        if ($r.outcome -ne 'added') { $pass = $false }
        $ids[$s.Key] = $r.id
        $folders[$s.Key] = $s.Folder
    }
    $d += '--- adb shell ls -R'
    $d += Sh "ls -R $ROOT"
    $rootList = (Sh "ls -1A $ROOT") -split "`n" | Sort-Object
    if (($rootList -join ',') -ne 'library.json,papers') { $pass = $false; $d += "根目錄內容不符：$($rootList -join ',')" }
    foreach ($s in $specs) {
        $files = (Sh "ls -1A '$ROOT/papers/$($s.Folder)'") -split "`n" | Sort-Object
        if (($files -join ',') -ne 'paper.json,source.pdf') { $pass = $false; $d += "$($s.Folder) 內容不符：$($files -join ',')" }
        $size = [int64](Sh "stat -c %s '$ROOT/papers/$($s.Folder)/source.pdf'")
        if ($size -ne $pdfSize[$s.File]) { $pass = $false; $d += "$($s.Folder)/source.pdf 大小 $size ≠ 來源 $($pdfSize[$s.File])" }
        else { $d += "$($s.Folder)/source.pdf 大小 $size = 來源" }
    }
    # 取出 alpha 的 paper.json，逐欄對照檔案格式
    $pj = "$ROOT/papers/$($folders['alpha'])/paper.json"
    $raw = Sh "cat '$pj'"
    $d += '--- adb shell cat paper.json（alpha）'
    $d += $raw
    $shape = '(?s)^\{\n  "format": 1,\n  "id": "[0-9A-HJKMNP-TV-Z]{26}",\n  "short_name": null,\n  "aliases": \[\],\n  "added": "\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z",\n  "bib": \{\n    "title": "Synthetic Thirty",\n    "authors": \[\n      \{\n        "family": "Alpha",\n        "given": null,\n        "literal": null\n      \}\n    \],\n    "year": 2024,\n    "venue": null,\n    "doi": null,\n    "url": null,\n    "modified": "\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z",\n    "modified_by": "[0-9A-HJKMNP-TV-Z]{26}"\n  \},\n  "materials": \[\n    \{\n      "id": "[0-9A-HJKMNP-TV-Z]{26}",\n      "role": "source",\n      "form": "pdf",\n      "path": "source.pdf",\n      "sha256": "[0-9a-f]{64}",\n      "bytes": \d+,\n      "pages": 30,\n      "pages_modified": "\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z",\n      "pages_modified_by": "[0-9A-HJKMNP-TV-Z]{26}",\n      "title": null,\n      "provenance": \{\n        "method": "added-by-user",\n        "time": "\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z",\n        "origin_name": "s30\.pdf",\n        "proofread": null,\n        "page_map": null,\n        "source_sha256": null\n      \}\n    \}\n  \],\n  "groups": \[\],\n  "groups_modified": null,\n  "groups_modified_by": null\n\}$'
    if ($raw -match $shape) { $d += '欄位、順序、縮排與檔案格式相符（逐字樣式比對）' } else { $pass = $false; $d += '欄位或排版與檔案格式不符' }
    $j = $raw | ConvertFrom-Json -DateKind String
    $sha = (Sh "sha256sum '$ROOT/papers/$($folders['alpha'])/source.pdf'") -split '\s+' | Select-Object -First 1
    if ($j.materials[0].sha256 -eq $sha -and [int64]$j.materials[0].bytes -eq $pdfSize['s30.pdf']) {
        $d += "sha256 與 bytes 和裝置上的檔案相符（$sha）"
    } else { $pass = $false; $d += "sha256/bytes 不符：記錄 $($j.materials[0].sha256) / $($j.materials[0].bytes)，裝置 $sha" }
    $tail = Sh "tail -c 1 '$pj' | od -An -tx1"
    $head = Sh "head -c 3 '$pj' | od -An -tx1"
    if ($tail.Trim() -eq '0a' -and $head.Trim() -ne 'ef bb bf') { $d += "結尾一個換行（0a），開頭沒有位元組順序記號（$($head.Trim())）" }
    else { $pass = $false; $d += "換行或位元組順序記號不符：結尾 [$($tail.Trim())] 開頭 [$($head.Trim())]" }
    $list = Pr 'list'
    if (@($list).Count -ne 3) { $pass = $false; $d += "list 筆數 $(@($list).Count) ≠ 3" } else { $d += 'list 筆數 3' }
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 2 ----------
Item 2 '加入 400MB 檔案途中 pkill -9，重開後沒有那一篇也沒有暫存資料夾' {
    $d = @()
    Sh "rm -f /data/local/tmp/prcli-bg.out" | Out-Null
    $bgCmd = "$P $ROOT $DEV add $SRC/prcli-400m.bin --title Big400 > /data/local/tmp/prcli-bg.out 2>&1"
    $proc = Start-Process -FilePath $adb -ArgumentList @('shell', ('"' + $bgCmd + '"')) -PassThru -WindowStyle Hidden
    # 模擬器的儲存很快，400MB 不到一秒就複製完，固定等 1 秒會錯過。改成在裝置上等暫存檔一出現就立刻 pkill -9，
    # 並記下那一刻暫存檔已經寫了多少位元組（要小於 400MB，才算真的死在複製途中）。
    $killCmd = 'n=0; while [ $n -lt 300000 ]; do set -- ' + "$ROOT/papers/.pr-adding-*/source.pdf" + '; if [ -e "$1" ]; then pkill -9 prcli; sz=$(stat -c %s "$1"); echo KILLED_AT_BYTES=$sz; break; fi; n=$((n+1)); done; echo LOOPS=$n'
    $killOut = Sh $killCmd
    Start-Sleep -Seconds 1
    if (-not $proc.HasExited) { $proc.WaitForExit(5000) | Out-Null }
    $km = [regex]::Match($killOut, 'KILLED_AT_BYTES=(\d+)')
    $killedAt = if ($km.Success) { [int64]$km.Groups[1].Value } else { -1 }
    $d += "強制結束的量測：$killOut"
    $d += "結束那一刻暫存檔 source.pdf 已寫 $killedAt 位元組（來源 419430400 位元組）"
    $during = Sh "ls -1A $ROOT/papers"
    $stage  = Sh "ls -ld $ROOT/papers/.pr-adding-* 2>&1; ls -l $ROOT/papers/.pr-adding-*/ 2>&1"
    $bgOut = Sh 'cat /data/local/tmp/prcli-bg.out'
    $afterKill = Sh "ls -1A $ROOT/papers"
    $d += "強制結束前 papers/ 內容：`n$during"
    $d += "暫存資料夾（ls -l）：`n$stage"
    $d += "背景程式的輸出（沒有 outcome 表示沒跑完就被結束；Killed 是 sh 回報的）：[$bgOut]"
    $d += "強制結束後、重新開啟前 papers/ 內容：`n$afterKill"
    $list = Pr 'list'
    $afterOpen = Sh "ls -1A $ROOT/papers"
    $d += "重新開啟（list）後 papers/ 內容：`n$afterOpen"
    $d += "list 筆數：$(@($list).Count)"
    $pass = ($killedAt -gt 0) -and ($killedAt -lt 419430400) -and ($during -match '\.pr-adding-') -and ($afterKill -match '\.pr-adding-') -and ($bgOut -notmatch 'outcome') `
        -and (@($list).Count -eq 3) -and ($afterOpen -notmatch '\.pr-') -and ($afterOpen -notmatch 'Big400')
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 3 ----------
Item 3 '用 adb 改資料夾名稱與 paper.json 的篇名，重新開啟後看到新內容' {
    $d = @()
    $new = 'Renamed By ADB'
    Sh "mv '$ROOT/papers/$($folders['gamma'])' '$ROOT/papers/$new'" | Out-Null
    $folders['gamma'] = $new
    Sh "sed -i 's/`"title`": `"Synthetic Five`"/`"title`": `"Edited By ADB`"/' '$ROOT/papers/$($folders['team'])/paper.json'" | Out-Null
    $d += 'adb shell mv 改 gamma 的資料夾名稱；adb shell sed 改 team 的 paper.json 篇名'
    $list = Pr 'list'
    $d += ($list | ForEach-Object { "$($_.id)  $($_.folder)  title=$($_.bib.title)" })
    $g = $list | Where-Object { $_.id -eq $ids['gamma'] }
    $t = $list | Where-Object { $_.id -eq $ids['team'] }
    $pass = (@($list).Count -eq 3) -and ($g.folder -eq $new) -and ($t.bib.title -eq 'Edited By ADB')
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 4 ----------
Item 4 'Android 上的重新掃描：等待期間改資料夾名稱、等長改篇名、加入新的一篇' {
    $d = @()
    $newName = 'Renamed Twice'
    $oldGamma = $folders['gamma']
    $teamPj = "$ROOT/papers/$($folders['team'])/paper.json"
    Sh 'rm -f /data/local/tmp/prcli-rescan.out' | Out-Null
    # 背景：開啟（掃描一次）、等 3 秒、呼叫 rescan()，印出差異
    $bgCmd = "$P $ROOT $DEV rescan-after 3000 > /data/local/tmp/prcli-rescan.out 2>&1"
    $proc = Start-Process -FilePath $adb -ArgumentList @('shell', ('"' + $bgCmd + '"')) -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 1
    # 等待期間做三件事。每件事都確認真的做成了，沒做成就判失敗（不能因為設定失敗而判通過）。
    Sh "mv '$ROOT/papers/$oldGamma' '$ROOT/papers/$newName'" | Out-Null
    $mvOk = (Sh "test -e '$ROOT/papers/$newName/paper.json' && ! test -e '$ROOT/papers/$oldGamma' && echo yes || echo no") -eq 'yes'
    $sizeBefore = Sh "wc -c < '$teamPj'"
    $mtimeBefore = Sh "stat -c %Y '$teamPj'"
    Sh "sed -i 's/`"title`": `"Edited By ADB`"/`"title`": `"Edited By XYZ`"/' '$teamPj'" | Out-Null
    $sizeAfter = Sh "wc -c < '$teamPj'"
    $mtimeAfter = Sh "stat -c %Y '$teamPj'"
    $sedOk = ((Sh "grep -c 'Edited By XYZ' '$teamPj'") -eq '1') -and ($sizeBefore -eq $sizeAfter)
    $add = Pr "add $SRC/rescan-new.bin --title 'Added During Rescan'"
    $ids['rescan'] = $add.id
    $d += "mv 成功：$mvOk；sed 等長改篇名成功：$sedOk（大小 $sizeBefore → $sizeAfter；修改時間秒數 $mtimeBefore → $mtimeAfter）；add → $($add | ConvertTo-Json -Compress)"
    if (-not $proc.HasExited) { $proc.WaitForExit(15000) | Out-Null }
    $out = Sh 'cat /data/local/tmp/prcli-rescan.out'
    $d += "rescan-after 3000 的輸出：$out"
    $diff = $null
    try { $diff = ConvertFrom-Json -InputObject $out -DateKind String } catch { $d += '輸出不是 JSON' }
    $folders['gamma'] = $newName
    $pass = $mvOk -and $sedOk -and ($add.outcome -eq 'added') -and ($null -ne $diff)
    if ($null -ne $diff) {
        $changed = @($diff.changed); $added = @($diff.added); $removed = @($diff.removed)
        $c1 = $changed -contains $ids['gamma']
        $c2 = $changed -contains $ids['team']
        $a1 = $added -contains $add.id
        $d += "changed 含改資料夾名稱的那篇：$c1；changed 含等長改篇名的那篇：$c2；added 含新加入的：$a1；removed 筆數 $($removed.Count)"
        if (-not $c2) { $d += '注意：等長的修改沒有被看見。這是實際結果，如實記錄。' }
        $pass = $pass -and $c1 -and $c2 -and $a1 -and ($removed.Count -eq 0)
    }
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 5 ----------
Item 5 '取得記號後用 sed 改篇名，再用舊記號 set-title → Conflict' {
    $d = @()
    $g = Pr "get $($ids['alpha'])"
    $token = $g.token
    if ($token -notmatch '^[0-9a-f]{64}$') { return @{ Pass = $false; Detail = @("取得的版本記號不是 64 個十六進位字元：[$token]") } }
    $pj = "$ROOT/papers/$($folders['alpha'])/paper.json"
    Sh "sed -i 's/`"title`": `"Synthetic Thirty`"/`"title`": `"Edited By Sed`"/' '$pj'" | Out-Null
    $r = Pr "set-title $($ids['alpha']) Mine --token $token"
    $exit = $script:LastExit
    $d += "set-title 輸出：$($r | ConvertTo-Json -Compress)（結束碼 $exit）"
    $disk = Sh "grep -n '`"title`"' '$pj'"
    $d += "磁碟上的 title 行：`n$disk"
    $pass = ($r.error -eq 'Conflict') -and ($exit -eq 1) -and ($disk -match 'Edited By Sed') -and ($disk -notmatch 'Mine')
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 6 ----------
Item 6 'stress-edit 100 次，最後篇名是 edit-100' {
    $d = @()
    $r = Pr "stress-edit $($ids['alpha']) 100"
    $d += "stress-edit → $($r | ConvertTo-Json -Compress)"
    $g = Pr "get $($ids['alpha'])"
    $d += "get 的篇名：$($g.paper.bib.title)"
    $files = Sh "ls -1A '$ROOT/papers/$($folders['alpha'])'"
    $d += "資料夾內容：`n$files"
    $pass = ($r.ok -eq 100) -and ($g.paper.bib.title -eq 'edit-100') -and ($files -notmatch '\.pr-')
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 7 ----------
Item 7 '加入篇名只差大小寫的兩篇，第二個資料夾加序號' {
    $d = @()
    $a = Pr "add $SRC/case1.bin --title 'Case Test'"
    $b = Pr "add $SRC/case2.bin --title 'case test'"
    $d += "add 1 → $($a | ConvertTo-Json -Compress)"
    $d += "add 2 → $($b | ConvertTo-Json -Compress)"
    $ls = Sh "ls -1 $ROOT/papers"
    $d += "papers/ 內容：`n$ls"
    $ci = Sh "ls -d '$ROOT/papers/CASE TEST' 2>&1"
    $d += "以全大寫名稱 ls：$ci（有結果表示這個儲存空間不分大小寫）"
    $names = $ls -split "`n"
    $pass = ($a.outcome -eq 'added') -and ($b.outcome -eq 'added') -and ($names -contains 'Case Test') -and ($names -contains 'case test (2)')
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 8 ----------
Item 8 '加入 53MB 檔案並記錄秒數，單獨量暫存資料夾改名' {
    $d = @()
    $addCmd = 's=$(date +%s%N); ' + "$P $ROOT $DEV add $SRC/prcli-53m.bin --title Timing53 --family Timing --year 2025" + '; e=$(date +%s%N); echo ELAPSED_MS=$(( (e - s) / 1000000 ))'
    $out = Sh $addCmd
    $d += "add 53MB 的輸出：`n$out"
    $m = [regex]::Match($out, 'ELAPSED_MS=(\d+)')
    $addMs = if ($m.Success) { [int64]$m.Groups[1].Value } else { -1 }
    $json = ($out -split "`n" | Where-Object { $_ -like '{*' } | Select-Object -First 1) | ConvertFrom-Json
    $ids['timing'] = $json.id
    $folders['timing'] = 'Timing 2025 - Timing53'
    $exists = Sh "ls -1 '$ROOT/papers/Timing 2025 - Timing53'"
    $d += "正式資料夾內容：$exists"

    # 同樣大小的資料夾，單獨量「.pr-adding-… 改成正式名稱」。放在隱藏資料夾，不留痕跡。
    $renCmd = "mkdir -p $ROOT/.timing/.pr-adding-x; " +
        'cs=$(date +%s%N); ' + "cp $SRC/prcli-53m.bin $ROOT/.timing/.pr-adding-x/source.pdf; " + 'ce=$(date +%s%N); ' +
        'rs=$(date +%s%N); ' + "mv $ROOT/.timing/.pr-adding-x $ROOT/.timing/Final; " + 're=$(date +%s%N); ' +
        "ls $ROOT/.timing/Final; " +
        'echo COPY_MS=$(( (ce - cs) / 1000000 )); echo RENAME_MS=$(( (re - rs) / 1000000 )); ' +
        "rm -rf $ROOT/.timing"
    $ren = Sh $renCmd
    $d += "單獨改名的量測：`n$ren"
    $rm = [regex]::Match($ren, 'RENAME_MS=(\d+)')
    $cm = [regex]::Match($ren, 'COPY_MS=(\d+)')
    $d += "加入 53MB 全程 $addMs 毫秒；其中複製到同類資料夾 $($cm.Groups[1].Value) 毫秒；資料夾改名 $($rm.Groups[1].Value) 毫秒（模擬器上的數字，只記錄）"
    $pass = ($json.outcome -eq 'added') -and ($exists -match 'source.pdf') -and $rm.Success -and ($ren -match 'source.pdf') -and $m.Success
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 9 ----------
Item 9 'remove → 再 add 同一份 → in_trash → restore' {
    $d = @()
    $r1 = Pr "remove $($ids['alpha'])"
    $d += "remove → $($r1 | ConvertTo-Json -Compress)"
    $r2 = Pr "add $SRC/s30.pdf --title Again"
    $d += "add 同一份 → $($r2 | ConvertTo-Json -Compress)"
    $trash = Sh "ls -1A $ROOT/.trash"
    $d += ".trash/ 內容：$trash"
    $r3 = Pr "restore $($ids['alpha'])"
    $d += "restore → $($r3 | ConvertTo-Json -Compress)"
    $list = Pr 'list'
    $back = $list | Where-Object { $_.id -eq $ids['alpha'] }
    $d += "還原後：$($back.id)  $($back.folder)"
    $trashAfter = Sh "ls -1A $ROOT/.trash"
    $d += "還原後 .trash/ 內容：[$trashAfter]"
    $pass = ($r1.ok -eq $true) -and ($r2.outcome -eq 'in_trash') -and ($r2.id -eq $ids['alpha']) `
        -and ($trash -match '^\d{8}T\d{6}Z-') -and ($r3.ok -eq $true) -and ($null -ne $back) -and ($back.folder -eq $folders['alpha']) -and ($trashAfter -eq '')
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 10 ----------
Item 10 '別的工具的隱藏檔（.stfolder、.obsidian、.DS_Store）在 init 與 list 之後原封不動' {
    $d = @()
    $pf = "$ROOT/papers/$($folders['alpha'])/.DS_Store"
    Sh "mkdir -p $ROOT/.stfolder $ROOT/.obsidian; echo '{`"a`":1}' > $ROOT/.obsidian/app.json; echo marker > $ROOT/.stfolder/marker.txt; echo binary > '$pf'" | Out-Null
    # 先確認隱藏項目真的建立成功；沒建立成功就判失敗，不能拿空的快照去比對而判通過
    $missing = @()
    foreach ($need in @("$ROOT/.stfolder", "$ROOT/.stfolder/marker.txt", "$ROOT/.obsidian", "$ROOT/.obsidian/app.json", $pf)) {
        if ((Sh "test -e '$need' && echo yes || echo no") -ne 'yes') { $missing += $need }
    }
    if ($missing.Count -gt 0) {
        return @{ Pass = $false; Detail = @("設定失敗，這些項目沒建立成功：$($missing -join '、')") }
    }
    $d += '已確認 .stfolder/、.stfolder/marker.txt、.obsidian/、.obsidian/app.json、.DS_Store 都存在'
    $snap = "cd $ROOT && find .stfolder .obsidian 'papers/$($folders['alpha'])/.DS_Store' | sort && find .stfolder .obsidian 'papers/$($folders['alpha'])/.DS_Store' -type f -exec sha256sum {} + && find .stfolder .obsidian 'papers/$($folders['alpha'])/.DS_Store' -exec stat -c '%n %s %Y' {} +"
    $before = Sh $snap
    $null = Pr 'init'
    $null = Pr 'list'
    $null = Pr 'check quick'
    $after = Sh $snap
    $d += "之前：`n$before"
    $d += "之後相同：$($before -eq $after)"
    if ($before -ne $after) { $d += "之後：`n$after" }
    @{ Pass = (($before -eq $after) -and ($before.Length -gt 0)); Detail = $d }
}

# ---------- 項目 11 ----------
Item 11 '寫入打開、位置、已讀各一筆，adb reboot，重開機後閱讀狀況與重開機前相同；events、check full' {
    $d = @()
    $a = $ids['alpha']
    $mid = (Pr "get $a").paper.materials[0].id
    $e1 = Pr "open-paper $a $mid"
    $e2 = Pr "position $a $mid 12 0.25 --zoom 1.5"
    $e3 = Pr "set-read $a true"
    $sent = @($e1.id, $e2.id, $e3.id)
    $types = @($e1.type, $e2.type, $e3.type) -join ','
    if (@($sent | Where-Object { $_ -match '^[0-9A-HJKMNP-TV-Z]{26}$' }).Count -ne 3 -or $types -ne 'open,position,status') {
        return @{ Pass = $false; Detail = @("事件沒有寫成功：$($sent -join ', ')（種類 $types）") }
    }
    $d += "寫入的事件：$($sent -join ', ')（種類 $types）"
    $p0 = Pr "progress $a"
    $p0json = ConvertTo-Json -InputObject $p0 -Depth 8 -Compress
    $d += "重開機前的閱讀狀況：$p0json"
    $okBefore = ($p0.status -eq 'Read') -and ($p0.read_at -eq $e3.t) -and ($p0.position.page -eq 12) -and ($p0.position.material -eq $mid)
    $l0 = Pr 'list'
    $n0 = @($l0).Count
    $chk0 = Pr 'check full'
    $d += "重開機前：list $n0 筆；check full → $(ConvertTo-Json -InputObject $chk0 -Compress)"
    $d += '--- adb reboot'
    & $adb reboot
    Start-Sleep -Seconds 10
    WaitBoot
    $d += '重開機完成'
    $p1 = Pr "progress $a"
    $p1json = ConvertTo-Json -InputObject $p1 -Depth 8 -Compress
    $l1 = Pr 'list'
    $n1 = @($l1).Count
    $inList = ConvertTo-Json -InputObject (@($l1 | Where-Object { $_.id -eq $a })[0].progress) -Depth 8 -Compress
    $ev = Pr 'events'
    $chk1 = Pr 'check full'
    $got = @($ev | ForEach-Object { $_.id })
    $d += "重開機後的閱讀狀況：$p1json"
    $d += "重開機後：list $n1 筆；events $(@($ev).Count) 筆（$($got -join ', ')）；check full → $(ConvertTo-Json -InputObject $chk1 -Compress)"
    $d += "重開機前後的閱讀狀況相同：$($p0json -ceq $p1json)；list 裡帶的閱讀狀況與單獨查的相同：$($inList -ceq $p1json)"
    $d += '--- adb shell ls -lR logs/'
    $d += Sh "ls -lR $ROOT/logs"
    $allSent = ($sent | Where-Object { $got -contains $_ }).Count -eq 3
    $pass = $okBefore -and ($p0json -ceq $p1json) -and ($inList -ceq $p1json) -and ($n1 -eq $n0) -and (@($ev).Count -eq 3) -and $allSent -and (@($chk1).Count -eq 0) -and (@($chk0).Count -eq 0)
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 12 ----------
Item 12 '移除一篇後列出回收區看得到它；還原後閱讀狀況仍在' {
    $d = @()
    $a = $ids['alpha']
    $name = $folders['alpha']
    $before = ConvertTo-Json -InputObject (Pr "progress $a") -Depth 8 -Compress
    $d += "移除前的閱讀狀況：$before"
    $r = Pr "remove $a"
    $trash = Pr 'trash-list'
    $d += "trash-list 輸出：$(ConvertTo-Json -InputObject $trash -Depth 8 -Compress)"
    $mine = @($trash | Where-Object { $_.paper_id -eq $a })
    $folderOnDisk = Sh "ls -1A $ROOT/.trash"
    $d += ".trash/ 內容：$folderOnDisk"
    $found = ($mine.Count -eq 1)
    $timeOk = $false
    if ($found) {
        $c = $mine[0].folder.Substring(0, 16)
        $iso = '{0}-{1}-{2}T{3}:{4}:{5}.000Z' -f $c.Substring(0,4), $c.Substring(4,2), $c.Substring(6,2), $c.Substring(9,2), $c.Substring(11,2), $c.Substring(13,2)
        $timeOk = ($mine[0].removed_at -eq $iso) -and ($mine[0].name -eq $name) -and ($mine[0].bib.title -eq 'edit-100')
        $d += "移除時間 $($mine[0].removed_at)，與資料夾名稱 $($mine[0].folder) 相符：$timeOk"
    }
    $inList = @(@(Pr 'list') | Where-Object { $_.id -eq $a }).Count
    $q = Pr "progress $a"
    $d += "移除後：list 裡有這篇的筆數 $inList；progress → $(ConvertTo-Json -InputObject $q -Compress)"
    $r2 = Pr "restore $a"
    $after = ConvertTo-Json -InputObject (Pr "progress $a") -Depth 8 -Compress
    $d += "還原後的閱讀狀況：$after"
    $trashAfter = Pr 'trash-list'
    $d += "還原後 trash-list 筆數：$(@($trashAfter).Count)"
    $pass = ($r.ok -eq $true) -and $found -and $timeOk -and ($inList -eq 0) -and ($q.error -eq 'NotFound') `
        -and ($r2.ok -eq $true) -and ($before -ceq $after) -and (@($trashAfter).Count -eq 0)
    @{ Pass = $pass; Detail = $d }
}

# ===================================================================================================
# 以下是核心全局功能的七個項目，編號順延成 13 到 19（這份腳本已有 11、12 兩項）。
# 在模擬器上另外會動 /sdcard/Documents/pr-backup*.zip 與 /sdcard/Documents/PaperReaderRestore*，收尾時刪掉。
# ===================================================================================================

$BK_DIR = '/sdcard/Documents'
$ISO = '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$'
$ULID = '^[0-9A-HJKMNP-TV-Z]{26}$'
function ShJson([string]$cmd) { ConvertFrom-Json -InputObject (Sh $cmd) -DateKind String }
function PrAt([string]$root, [string]$argline) {
    $txt = Sh "$P $root $DEV $argline"
    try { return , (ConvertFrom-Json -InputObject $txt -DateKind String -NoEnumerate) }
    catch { throw "prcli 的輸出不是 JSON：$txt" }
}
# 在裝置上量一個指令花的毫秒數；交回 @{ Ms; Json }（Json 是指令輸出的最後一行 JSON）
function TimedPr([string]$root, [string]$argline) {
    $cmd = 's=$(date +%s%N); ' + "$P $root $DEV $argline > /data/local/tmp/prcli-timed.out 2>&1; " + 'e=$(date +%s%N); echo ELAPSED_MS=$(( (e - s) / 1000000 )); cat /data/local/tmp/prcli-timed.out'
    $out = Sh $cmd
    $ms = [regex]::Match($out, 'ELAPSED_MS=(\d+)').Groups[1].Value
    $line = @($out -split "`n" | Where-Object { $_.StartsWith('{') -or $_.StartsWith('[') }) | Select-Object -Last 1
    $json = $null
    if ($line) { $json = ConvertFrom-Json -InputObject $line -DateKind String }
    @{ Ms = [int64]$ms; Json = $json; Raw = $out }
}

# ---------- 項目 13----------
Item 13 '建兩個群組、把兩篇加入第一個；取出 library.json 與 paper.json，內容與格式說明相符' {
    $d = @()
    $devId = (Sh "cat $DEV").Trim()
    $g1 = (Pr 'group-create Vision').group
    $g2 = (Pr 'group-create Detection').group
    $ids['g1'] = $g1.id
    $ids['g2'] = $g2.id
    $d += "建立群組：$($g1.id) Vision；$($g2.id) Detection"
    $null = Pr "group-add $($ids['alpha']) $($g1.id)"
    $null = Pr "group-add $($ids['team']) $($g1.id)"
    $lib = ShJson "cat $ROOT/library.json"
    $pj = ShJson "cat '$ROOT/papers/$($folders['alpha'])/paper.json'"
    $d += "library.json：`n$(Sh "cat $ROOT/library.json")"
    $d += "alpha 的 paper.json 的 groups 三個欄位：$(ConvertTo-Json -InputObject @($pj.groups, $pj.groups_modified, $pj.groups_modified_by) -Compress)"
    $defOk = (@($lib.groups).Count -eq 2)
    foreach ($g in @($lib.groups)) {
        $defOk = $defOk -and (@($g.PSObject.Properties).Count -eq 5) -and ($g.id -match $ULID) -and ($g.created -match $ISO) -and ($g.modified -match $ISO) -and ($g.modified_by -eq $devId)
    }
    $names = @($pj.PSObject.Properties.Name)
    $order = ($names.IndexOf('groups') -eq $names.IndexOf('materials') + 1) -and ($names.IndexOf('groups_modified_by') -eq $names.IndexOf('materials') + 3)
    $memberOk = (@($pj.groups).Count -eq 1) -and ($pj.groups[0] -eq $g1.id) -and ($pj.groups_modified -match $ISO) -and ($pj.groups_modified_by -eq $devId)
    $listGroups = @((Pr 'list') | Where-Object { $_.id -eq $ids['alpha'] })[0].groups
    $d += "library.json 的定義各五個欄位、時間與裝置正確：$defOk；paper.json 欄位順序：$order；所屬與時間、裝置：$memberOk；list 帶的 groups：$(ConvertTo-Json -InputObject @($listGroups) -Compress)"
    $pass = $defOk -and $order -and $memberOk -and (@($lib.deleted_groups).Count -eq 0) -and ($lib.notes_dir -eq 'notes') -and (@($listGroups).Count -eq 1)
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 14----------
Item 14 '刪掉第一個群組：兩篇的 groups 都空了；library.json 只剩第二個群組、deleted_groups 有一筆' {
    $d = @()
    $devId = (Sh "cat $DEV").Trim()
    $r = Pr "group-delete $($ids['g1'])"
    $d += "group-delete → $(ConvertTo-Json -InputObject $r -Compress)"
    $lib = ShJson "cat $ROOT/library.json"
    $pa = ShJson "cat '$ROOT/papers/$($folders['alpha'])/paper.json'"
    $pt = ShJson "cat '$ROOT/papers/$($folders['team'])/paper.json'"
    $d += "library.json 的 groups：$(ConvertTo-Json -InputObject @($lib.groups) -Compress -Depth 5)"
    $d += "library.json 的 deleted_groups：$(ConvertTo-Json -InputObject @($lib.deleted_groups) -Compress -Depth 5)"
    $pass = (@($r.skipped).Count -eq 0) -and (@($pa.groups).Count -eq 0) -and (@($pt.groups).Count -eq 0) `
        -and (@($lib.groups).Count -eq 1) -and ($lib.groups[0].id -eq $ids['g2']) `
        -and (@($lib.deleted_groups).Count -eq 1) -and ($lib.deleted_groups[0].id -eq $ids['g1']) -and ($lib.deleted_groups[0].deleted -match $ISO) -and ($lib.deleted_groups[0].deleted_by -eq $devId)
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 15----------
Item 15 '使用者自己的 Translation.pdf 在資料夾裡，再 add-translation 一份內容不同的：得到 translation-2.pdf，原檔不動' {
    $d = @()
    $f = "$ROOT/papers/$($folders['team'])"
    # 使用者自己放的檔（沒列在材料清單）。/sdcard 不分大小寫，所以 translation.pdf 算被占用
    Sh "cp $SRC/s5.pdf '$f/Translation.pdf'" | Out-Null
    $shaBefore = (Sh "sha256sum '$f/Translation.pdf'") -split '\s+' | Select-Object -First 1
    $r = Pr "add-translation $($ids['team']) $SRC/case1.bin --pages 7"
    $d += "add-translation → $(ConvertTo-Json -InputObject $r -Compress)"
    $ls = Sh "ls -1A '$f'"
    $d += "資料夾內容：`n$ls"
    $shaAfter = (Sh "sha256sum '$f/Translation.pdf'") -split '\s+' | Select-Object -First 1
    $pj = ShJson "cat '$f/paper.json'"
    $m = @($pj.materials)[-1]
    $d += "最後一項材料：$(ConvertTo-Json -InputObject $m -Compress -Depth 5)"
    $pass = ($r.outcome -eq 'added') -and ($ls -match 'translation-2\.pdf') -and ($shaBefore -eq $shaAfter) -and ($shaBefore.Length -eq 64) `
        -and ($m.role -eq 'translation') -and ($m.path -eq 'translation-2.pdf') -and ($m.pages -eq 7)
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 16----------
Item 16 '加兩條劃線、改一條的短評、寫想法；筆記檔與格式說明相符，adb pull 回電腦後文字編輯器看得懂' {
    $d = @()
    $a = $ids['alpha']
    $mid = (Pr "get $a").paper.materials[0].id
    $h1 = (Pr "highlight-add $a $mid --page 0 --rect 72,500,300,512 --rect 72,486,180,498 --quote 'first quote' --comment 'first comment' --label 1").highlight
    $h2 = (Pr "highlight-add $a $mid --page 2 --rect 10,20,30,40 --quote 'second quote'").highlight
    $null = Pr "highlight-comment $a $($h2.id) 'edited comment'"
    $null = Pr "thoughts $a 'overall thoughts'"
    $n = Pr "notes $a"
    $path = "$ROOT/$($n.path)"
    $text = Sh "cat '$path'"
    $d += "筆記檔 $($n.path)：`n$text"
    $local = Join-Path $appDir 'target\emulator-notes.md'
    & $adb pull $path $local | Out-Null
    $pulled = Get-Content -LiteralPath $local -Raw -Encoding UTF8
    $d += "adb pull 回電腦：$local（$($pulled.Length) 個字元）"
    $okText = ($text -match '(?m)^---$') -and ($text -match "paperreader_paper: $a") -and ($text -match 'paperreader_modified_by: ') `
        -and ($text -match '<!-- paperreader:thoughts -->') -and ($text -match '<!-- paperreader:highlights -->') `
        -and ($text -match '(?m)^<!-- paperreader:highlight \{"v":1,') -and ($text -match '<!-- paperreader:highlight-end -->') `
        -and ($text -match "\^$($h1.id)") -and ($text -match "\^$($h2.id)") -and ($text -match '> first quote') `
        -and ($text -match 'edited comment') -and ($text -notmatch 'second comment') -and ($text -match 'overall thoughts') `
        -and ($text -match '\[Open in PaperReader p\. 1\]\(paperreader://open\?paper=')
    $okPull = ($pulled.Replace("`r`n", "`n").TrimEnd() -eq $text.Replace("`r`n", "`n").TrimEnd())
    $d += "文字符合規格樣子：$okText；拉回電腦的內容與裝置上相同：$okPull"
    @{ Pass = ($okText -and $okPull -and (@($n.highlights).Count -eq 2)); Detail = $d }
}

# ---------- 項目 17----------
$R2 = "$BK_DIR/PaperReaderRestore"
$BK1 = "$BK_DIR/pr-backup.zip"
Item 17 '匯出備份檔（記秒數）、還原到另一個位置（記秒數）；兩個文獻庫的 list、groups、notes、check full 相同' {
    $d = @()
    Sh "rm -rf $BK1 $R2" | Out-Null
    $ex = TimedPr $ROOT "export-backup $BK1"
    $d += "export-backup → $(ConvertTo-Json -InputObject $ex.Json -Compress -Depth 5)；$($ex.Ms) ms（含 53MB 那份；只記錄，不判定）"
    $size = Sh "stat -c %s $BK1"
    $d += "備份檔大小：$size 位元組"
    $rs = TimedPr $R2 "restore-backup $BK1"
    $d += "restore-backup → manifest_mismatches=$(ConvertTo-Json -InputObject @($rs.Json.manifest_mismatches) -Compress)；problems=$(ConvertTo-Json -InputObject @($rs.Json.problems) -Compress -Depth 5)；$($rs.Ms) ms（只記錄）"
    $same = @{}
    foreach ($c in @('list', 'groups', 'trash-list', 'check full')) {
        $x = ConvertTo-Json -InputObject (Pr $c) -Depth 20 -Compress
        $y = ConvertTo-Json -InputObject (PrAt $R2 $c) -Depth 20 -Compress
        $same[$c] = ($x -ceq $y)
    }
    foreach ($paperId in @((Pr 'list') | ForEach-Object { $_.id })) {
        $x = ConvertTo-Json -InputObject (Pr "notes $paperId") -Depth 20 -Compress
        $y = ConvertTo-Json -InputObject (PrAt $R2 "notes $paperId") -Depth 20 -Compress
        $same["notes $paperId"] = ($x -ceq $y)
    }
    $d += "兩邊相同：$(ConvertTo-Json -InputObject $same -Compress)"
    $pass = ($null -ne $ex.Json) -and ($ex.Json.files -gt 0) -and (@($ex.Json.skipped).Count -eq 0) -and ($null -ne $rs.Json) `
        -and (@($rs.Json.manifest_mismatches).Count -eq 0) -and (@($same.Values | Where-Object { -not $_ }).Count -eq 0)
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 18----------
$BK2 = "$BK_DIR/pr-backup-big.zip"
Item 18 '先加 400MB 的合成 PDF；背景匯出，看到 .pr-export-*.zip 出現就 pkill -9；再匯出一次到同一個資料夾' {
    $d = @()
    # 另外產生一份新的 400MB 檔：項目 2 的那份若在被殺掉之前其實已經加入完成，再加同一份會回 already_present
    Sh "dd if=/dev/urandom of=$SRC/prcli-400m-b.bin bs=1M count=400 2>&1 | tail -1; printf '%%PDF-' | dd of=$SRC/prcli-400m-b.bin conv=notrunc 2>&1 | tail -1" | Out-Null
    $add = Pr "add $SRC/prcli-400m-b.bin --title Big400Export"
    $d += "加入 400MB → $(ConvertTo-Json -InputObject $add -Compress)"
    Sh "rm -f $BK2 /data/local/tmp/prcli-bg.out; rm -f $BK_DIR/.pr-export-*" | Out-Null
    $bgCmd = "$P $ROOT $DEV export-backup $BK2 > /data/local/tmp/prcli-bg.out 2>&1"
    $proc = Start-Process -FilePath $adb -ArgumentList @('shell', ('"' + $bgCmd + '"')) -PassThru -WindowStyle Hidden
    # 模擬器的儲存很快，固定等一秒會錯過：在裝置上輪詢，暫存檔一出現就立刻 pkill -9（照項目 2 的做法）
    $killCmd = 'n=0; while [ $n -lt 300000 ]; do set -- ' + "$BK_DIR/.pr-export-*.zip" + '; if [ -e "$1" ]; then pkill -9 prcli; sz=$(stat -c %s "$1"); echo KILLED_AT_BYTES=$sz; break; fi; n=$((n+1)); done; echo LOOPS=$n'
    $killOut = Sh $killCmd
    Start-Sleep -Seconds 1
    if (-not $proc.HasExited) { $proc.WaitForExit(5000) | Out-Null }
    $d += "強制結束的量測：$killOut"
    $left = Sh "ls -1A $BK_DIR | grep '^.pr-export-' ; echo END"
    $outExists = Sh "test -e $BK2 && echo yes || echo no"
    $d += "強制結束後 $BK_DIR 裡的暫存檔：`n$left；目標檔存在：$outExists"
    $ex = TimedPr $ROOT "export-backup $BK2"
    $left2 = Sh "ls -1A $BK_DIR | grep '^.pr-export-' ; echo END"
    $d += "再匯出一次 → $(ConvertTo-Json -InputObject $ex.Json -Compress -Depth 5)；之後的暫存檔：`n$left2"
    $pass = ($add.outcome -eq 'added') -and ($killOut -match 'KILLED_AT_BYTES=\d+') -and ($left -match '\.pr-export-.*\.zip') -and ($outExists -eq 'no') `
        -and ($null -ne $ex.Json) -and ($ex.Json.files -gt 0) -and ($left2 -notmatch '\.pr-export-') -and ((Sh "test -e $BK2 && echo yes || echo no") -eq 'yes')
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 19----------
Item 19 '背景還原（用項目 18 的備份），看到 .pr-restoring-* 出現就 pkill -9；目標不是半個文獻庫；再還原一次成功' {
    $d = @()
    $R3 = "$BK_DIR/PaperReaderRestore2"
    Sh "rm -rf $R3 $BK_DIR/.pr-restoring-*; rm -f /data/local/tmp/prcli-bg.out" | Out-Null
    $bgCmd = "$P $R3 $DEV restore-backup $BK2 > /data/local/tmp/prcli-bg.out 2>&1"
    $proc = Start-Process -FilePath $adb -ArgumentList @('shell', ('"' + $bgCmd + '"')) -PassThru -WindowStyle Hidden
    $killCmd = 'n=0; while [ $n -lt 300000 ]; do set -- ' + "$BK_DIR/.pr-restoring-*" + '; if [ -e "$1" ]; then pkill -9 prcli; echo KILLED; break; fi; n=$((n+1)); done; echo LOOPS=$n'
    $killOut = Sh $killCmd
    Start-Sleep -Seconds 1
    if (-not $proc.HasExited) { $proc.WaitForExit(5000) | Out-Null }
    $d += "強制結束的量測：$killOut"
    $stage = Sh "ls -1A $BK_DIR | grep '^.pr-restoring-' ; echo END"
    $targetState = Sh "if [ ! -e $R3 ]; then echo missing; elif [ -z `"`$(ls -A $R3)`" ]; then echo empty; else echo has-content; fi"
    $d += "強制結束後：暫存資料夾：`n$stage；目標 $R3 的狀態：$targetState（要是 missing 或 empty，不能是半個文獻庫）"
    $rs = TimedPr $R3 "restore-backup $BK2"
    $stage2 = Sh "ls -1A $BK_DIR | grep '^.pr-restoring-' ; echo END"
    $d += "再還原一次 → manifest_mismatches=$(ConvertTo-Json -InputObject @($rs.Json.manifest_mismatches) -Compress)；之後的暫存資料夾：`n$stage2"
    $pass = ($killOut -match 'KILLED') -and ($targetState -ne 'has-content') -and ($null -ne $rs.Json) -and (@($rs.Json.manifest_mismatches).Count -eq 0) `
        -and ($rs.Json.files -gt 0) -and ($stage2 -notmatch '\.pr-restoring-')
    @{ Pass = $pass; Detail = $d }
}

# ===================================================================================================
# 以下是同步包的六個項目，編號 20 到 25。
# 
# 用兩個根目錄模擬兩台裝置，裝置代號是兩個固定值；另外會動 /sdcard/Documents/pr-sync*.zip，收尾時刪掉。
# ===================================================================================================
$SA = "$BK_DIR/PaperReaderSyncA"
$SB = "$BK_DIR/PaperReaderSyncB"
$DEVA = '/data/local/tmp/prcli-device-sync-a'
$DEVB = '/data/local/tmp/prcli-device-sync-b'
Sh "rm -rf $SA $SB $BK_DIR/pr-sync*.zip; echo 01JSYNCA00000000000000000A > $DEVA; echo 01JSYNCB00000000000000000B > $DEVB" | Out-Null
# 這幾項要同時放好幾份 400MB 的檔（來源、兩個文獻庫、同步包、匯入時的暫存），模擬器的 /sdcard 只有 7.7 GB：
# 先刪掉項目 2、17 到 19 留下、之後用不到的大檔；各項也在大檔用完時就刪。
Sh "rm -rf $BK_DIR/pr-backup*.zip $BK_DIR/PaperReaderRestore* $SRC/prcli-400m.bin $SRC/prcli-400m-b.bin" | Out-Null
function PrD([string]$root, [string]$dev, [string]$argline) {
    $txt = Sh "$P $root $dev $argline"
    try { return , (ConvertFrom-Json -InputObject $txt -DateKind String -NoEnumerate) }
    catch { throw "prcli 的輸出不是 JSON：$txt" }
}
function PA([string]$a) { PrD $SA $DEVA $a }
function PB([string]$a) { PrD $SB $DEVB $a }
function TimedD([string]$root, [string]$dev, [string]$argline) {
    $cmd = 's=$(date +%s%N); ' + "$P $root $dev $argline > /data/local/tmp/prcli-timed.out 2>&1; " + 'e=$(date +%s%N); echo ELAPSED_MS=$(( (e - s) / 1000000 )); cat /data/local/tmp/prcli-timed.out'
    $out = Sh $cmd
    $ms = [regex]::Match($out, 'ELAPSED_MS=(\d+)').Groups[1].Value
    $line = @($out -split "`n" | Where-Object { $_.StartsWith('{') -or $_.StartsWith('[') }) | Select-Object -Last 1
    $json = $null
    if ($line) { $json = ConvertFrom-Json -InputObject $line -DateKind String }
    @{ Ms = [int64]$ms; Json = $json; Raw = $out }
}
# 裝置上一個檔的大小；不存在時是 -1（不讓轉換數字的例外蓋掉真正的原因）
function SizeOf([string]$path) { [int64](Sh "stat -c %s '$path' 2>/dev/null || echo -1") }
# prcli 交回的是不是錯誤（{"error":…}）；空的也算
function IsErr($v) { ($null -eq $v) -or ($null -ne $v.error) }
function Brief($v) { ConvertTo-Json -InputObject $v -Compress -Depth 6 }
# PrD 交回的陣列是「包了一層」的（`return , …`），指定給變數之後才是一般的陣列；
# 直接放進 @(…) 或管線會變成「一個元素、內容是整個陣列」（例如 @(PA 'check full').Count 永遠是 1）。
# 所以凡是要數個數、排序、篩選的，一律先指定給變數再用。
# 兩邊的 list（去掉資料夾名稱與閱讀狀況）、trash-list（去掉資料夾名稱）、每篇的 notes（去掉路徑與時間）、ink 是否相同
function SameBoth {
    $same = @{}
    $norm = { param($v) ConvertTo-Json -InputObject $v -Depth 20 -Compress }
    $la0 = PA 'list'; $lb0 = PB 'list'
    $la = @($la0 | Sort-Object id | ForEach-Object { $_.PSObject.Properties.Remove('folder'); $_.PSObject.Properties.Remove('progress'); $_ })
    $lb = @($lb0 | Sort-Object id | ForEach-Object { $_.PSObject.Properties.Remove('folder'); $_.PSObject.Properties.Remove('progress'); $_ })
    $same['list'] = ((& $norm $la) -ceq (& $norm $lb))
    $ta0 = PA 'trash-list'; $tb0 = PB 'trash-list'
    $ta = @($ta0 | Sort-Object paper_id | ForEach-Object { $_.paper_id + '|' + $_.name })
    $tb = @($tb0 | Sort-Object paper_id | ForEach-Object { $_.paper_id + '|' + $_.name })
    $same['trash'] = ((& $norm $ta) -ceq (& $norm $tb))
    $same['groups'] = ((& $norm (PA 'groups')) -ceq (& $norm (PB 'groups')))
    foreach ($pp in @($la | ForEach-Object { $_.id })) {
        $na = PA "notes $pp"; $nb = PB "notes $pp"
        foreach ($n in @($na, $nb)) { foreach ($k in @('path', 'modified', 'modified_by', 'token')) { $n.PSObject.Properties.Remove($k) } }
        $same["notes $pp"] = ((& $norm $na) -ceq (& $norm $nb))
        $ia = PA "ink $pp"; $ib = PB "ink $pp"
        # 變數名稱不可以用 $sa、$sb：PowerShell 的變數名稱不分大小寫、函式看得到呼叫者的變數，
        # 會蓋掉 $SA、$SB（兩個文獻庫的根目錄），下一輪的 PA、PB 就開到別的地方去
        $strokesA = @($ia.strokes | ForEach-Object { $_.id }) -join ','
        $strokesB = @($ib.strokes | ForEach-Object { $_.id }) -join ','
        $same["ink $pp"] = ($strokesA -ceq $strokesB)
    }
    $same
}

# ---------- 項目 20----------
Item 20 'A 加兩篇、建群組、加劃線與想法；sync-export 完整；B 用 sync-join 加入；兩邊相同、library.json 的 id 相同' {
    $d = @()
    $p1 = PA "add $SRC/s30.pdf --title SyncOne"
    $p2 = PA "add $SRC/s5.pdf --title SyncTwo --pages 10"
    $ids['s1'] = $p1.id; $ids['s2'] = $p2.id
    $g = (PA 'group-create SyncGroup').group
    $null = PA "group-add $($p1.id) $($g.id)"
    $mid = (PA "get $($p2.id)").paper.materials[0].id
    $ids['s2m'] = $mid
    $h = (PA "highlight-add $($p2.id) $mid --page 1 --rect 10,20,30,40 --quote 'sync quote' --comment 'from A'").highlight
    $ids['sh'] = $h.id
    $null = PA "thoughts $($p2.id) 'A thoughts'"
    $full = "$BK_DIR/pr-sync-full.zip"
    $ex = PA "sync-export $full --full --label EmuA"
    $d += "sync-export → mode=$($ex.mode) listed=$($ex.listed) included=$($ex.included)"
    $j = PB "sync-join $full"
    $d += "sync-join → files=$($j.files) mismatches=$(@($j.manifest_mismatches).Count) problems=$(@($j.problems).Count)"
    $same = SameBoth
    $idA = (PA 'init').library_id; $idB = (PB 'init').library_id
    $d += "兩邊相同：$(ConvertTo-Json -InputObject $same -Compress)；library id：$idA / $idB"
    $pass = ($ex.mode -eq 'full') -and (@($j.manifest_mismatches).Count -eq 0) -and ($idA -eq $idB) -and (@($same.Values | Where-Object { -not $_ }).Count -eq 0)
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 21----------
# 匯出用預設的模式（只帶變更）。B 是用 sync-join 加入的，兩邊都還沒有「對方匯入過我的同步包」的記錄，
# 這一輪兩份實際都是完整的（回報的 mode 是 full）；只帶變更的同步包由項目 25 驗。
# A 的變更都是刪除，所以 B 匯入的回報列在 removed（不在 updated）。
Item 21 'A 刪劃線、清一頁筆跡、移除一篇；B 改同一篇的書目、加一條劃線；互相 sync-export（預設的模式）、sync-import --defaults' {
    $d = @()
    $p1 = $ids['s1']; $p2 = $ids['s2']; $mid = $ids['s2m']
    $null = PA "highlight-delete $p2 $($ids['sh'])"
    $null = PA "stroke-add $p2 $mid --page 0 --color '#ff0000' --width 2 --points '1,1;2,2'"
    $null = PA "ink-clear-page $p2 $mid --page 0"
    $null = PA "remove $p1"
    $null = PB "set-title $p1 'Renamed on B'"
    $null = PB "highlight-add $p2 $mid --page 2 --rect 1,2,3,4 --quote 'B quote' --comment 'from B'"
    $ea = "$BK_DIR/pr-sync-a1.zip"; $eb = "$BK_DIR/pr-sync-b1.zip"
    $x1 = PA "sync-export $ea"
    $i1 = PB "sync-import $ea --defaults"
    $x2 = PB "sync-export $eb"
    $i2 = PA "sync-import $eb --defaults"
    $d += "A 匯出 mode=$($x1.mode) since=$($x1.since)；B 匯入 added=$(Brief @($i1.added)) updated=$(Brief @($i1.updated)) removed=$(Brief @($i1.removed)) conflicts=$(Brief @($i1.conflict_copies))"
    $d += "B 匯出 mode=$($x2.mode) since=$($x2.since)；A 匯入 added=$(Brief @($i2.added)) updated=$(Brief @($i2.updated)) removed=$(Brief @($i2.removed)) conflicts=$(Brief @($i2.conflict_copies))"
    $same = SameBoth
    $notes = PA "notes $p2"
    $back = @($notes.highlights | Where-Object { $_.id -eq $ids['sh'] }).Count
    $ca = PA 'check full'; $cb = PB 'check full'
    $d += "兩邊相同：$(Brief $same)；刪掉的劃線回來了：$($back -gt 0)；check full：A $(@($ca).Count) 項 $(Brief $ca)、B $(@($cb).Count) 項 $(Brief $cb)"
    $trashA = PA 'trash-list'
    $pass = -not ((IsErr $x1) -or (IsErr $i1) -or (IsErr $x2) -or (IsErr $i2)) `
        -and (@($same.Values | Where-Object { -not $_ }).Count -eq 0) -and ($back -eq 0) -and (@($ca).Count -eq 0) -and (@($cb).Count -eq 0) `
        -and (@($trashA | Where-Object { $_.paper_id -eq $p1 }).Count -eq 1)
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 22----------
Item 22 '兩邊各改同一篇的想法，互相匯入（先 --dry-run）；選 both 之後有衝突副本，adb pull 回電腦看得懂' {
    $d = @()
    $p2 = $ids['s2']
    $null = PA "thoughts $p2 'A changed again'"
    $null = PB "thoughts $p2 'B changed too'"
    $ea = "$BK_DIR/pr-sync-a2.zip"
    $null = PA "sync-export $ea"
    $dry = PB "sync-import $ea --dry-run"
    $kinds = @($dry.conflicts | ForEach-Object { $_.kind })
    $d += "預覽的衝突：$(ConvertTo-Json -InputObject $kinds -Compress)"
    $c = @($dry.conflicts | Where-Object { $_.kind -eq 'Thoughts' })[0]
    $r = PB "sync-import $ea --choose $($c.id)=both"
    $copy = @($r.conflict_copies)[0]
    $local = Join-Path $appDir 'target\emulator-conflict-copy.md'
    & $adb pull "$SB/$copy" $local | Out-Null
    $text = Get-Content -LiteralPath $local -Raw -Encoding UTF8
    $d += "衝突副本 $copy（拉回 $local）：`n$text"
    $pass = ($kinds.Count -eq 1) -and ($kinds[0] -eq 'Thoughts') -and ($null -ne $copy) -and ($text -match "paperreader_conflict_of: $p2") -and ($text -match 'A changed again')
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 23、24----------
# 背景匯入一份含 400MB 合成 PDF 的完整同步包（有一篇同時改內容又移到回收區）；看到 .pr-sync-*/plan.json 出現就 pkill -9。
# 打得中的原因：計畫的順序是「附加記錄 → 新的文獻 → 搬移 → 寫入 → 收尾」，新的那篇 400MB 在改名之前要重算一次指紋，
# 計畫檔寫成之後到寫筆記檔之前約有一秒（重開時做同樣的事，list 花 0.6 到 1.3 秒）。輪詢每 5000 次看一下 prcli 還在不在：
# 匯入出錯先結束了，就不空等。
# 交回 @{ Out = 輪詢與背景匯入的輸出; Killed = 是否打斷在計畫檔之後; PlanLeft = 打斷之後計畫檔是否還在 }
function KillDuringImport([string]$pkg) {
    Sh "rm -f /data/local/tmp/prcli-bg.out" | Out-Null
    $bgCmd = "$P $SB $DEVB sync-import $pkg --defaults > /data/local/tmp/prcli-bg.out 2>&1"
    $proc = Start-Process -FilePath $adb -ArgumentList @('shell', ('"' + $bgCmd + '"')) -PassThru -WindowStyle Hidden
    $killCmd = 'n=0; while [ $n -lt 600000 ]; do set -- ' + "$SB/.pr-sync-*/plan.json" + '; if [ -e "$1" ]; then pkill -9 prcli; echo KILLED; break; fi; n=$((n+1)); if [ $((n % 5000)) -eq 0 ] && ! pgrep -x prcli >/dev/null; then echo NOT_RUNNING; break; fi; done; echo LOOPS=$n'
    $out = Sh $killCmd
    Start-Sleep -Seconds 1
    if (-not $proc.HasExited) { $proc.WaitForExit(5000) | Out-Null }
    $plan = Sh "ls $SB/.pr-sync-*/plan.json >/dev/null 2>&1 && echo yes || echo no"
    $bg = Sh "cat /data/local/tmp/prcli-bg.out"
    @{ Out = "$out；計畫檔還在：$plan；背景匯入的輸出：$bg"; Killed = ($out -match 'KILLED'); PlanLeft = ($plan -eq 'yes') }
}
Item 23 '背景 sync-import（400MB、同一篇改內容又移到回收區），plan.json 出現就 pkill -9；再執行 list 把計畫做完' {
    $d = @()
    Sh "dd if=/dev/urandom of=$SRC/prcli-400m-sync.bin bs=1M count=400 2>&1 | tail -1; printf '%%PDF-' | dd of=$SRC/prcli-400m-sync.bin conv=notrunc 2>&1 | tail -1" | Out-Null
    $big = PA "add $SRC/prcli-400m-sync.bin --title Big400Sync"
    Sh "rm -f $SRC/prcli-400m-sync.bin" | Out-Null
    $ids['big'] = $big.id
    $p2 = $ids['s2']
    $null = PA "set-title $p2 'Moved after edit'"
    $null = PA "remove $p2"
    $pkg = "$BK_DIR/pr-sync-big.zip"
    $xa = PA "sync-export $pkg --full"
    $d += "A 加入 400MB → $(Brief $big)；匯出 → $(Brief $xa)"
    $kill = KillDuringImport $pkg
    $d += "強制結束：$($kill.Out)"
    $before = Sh "ls -1A $SB | grep '^.pr-sync-' ; echo END"
    $t = TimedD $SB $DEVB 'list'
    $after = Sh "ls -1A $SB | grep '^.pr-sync-' ; echo END"
    Sh "rm -f $pkg" | Out-Null
    $same = SameBoth
    $trashB = PB 'trash-list'
    $d += "重開前的暫存：$before；重開（list）$($t.Ms) ms（只記錄）；之後的暫存：$after；兩邊相同：$(Brief $same)"
    $pass = ($big.outcome -eq 'added') -and -not (IsErr $xa) -and $kill.Killed -and $kill.PlanLeft -and ($after -notmatch '\.pr-sync-') `
        -and (@($same.Values | Where-Object { -not $_ }).Count -eq 0) -and (@($trashB | Where-Object { $_.paper_id -eq $p2 }).Count -eq 1)
    @{ Pass = $pass; Detail = $d }
}

Item 24 '同 23，但中斷後先用 adb shell 改那篇的筆記檔，再執行 list：改過的筆記檔沒有被覆寫，旁邊多一份衝突副本' {
    $d = @()
    # 用還在 papers/ 的那一篇（項目 23 加的）；先讓 B 有它的筆記檔，並在背景匯入之前記下路徑
    $q = $ids['big']
    $null = PA "thoughts $q 'A first version'"
    $first = "$BK_DIR/pr-sync-q1.zip"
    $x1 = PA "sync-export $first"
    $i1 = PB "sync-import $first --defaults"
    Sh "rm -f $first" | Out-Null
    $notesRel = (PB "notes $q").path
    $notesPath = "$SB/$notesRel"
    $stem = [IO.Path]::GetFileNameWithoutExtension($notesRel)
    $d += "A 匯出 → mode=$($x1.mode) bytes=$($x1.bytes)；B 匯入 → $(if (IsErr $i1) { Brief $i1 } else { 'OK' })；B 的筆記檔：$notesPath"
    $null = PA "thoughts $q 'A second version'"
    Sh "dd if=/dev/urandom of=$SRC/prcli-400m-sync2.bin bs=1M count=400 2>&1 | tail -1; printf '%%PDF-' | dd of=$SRC/prcli-400m-sync2.bin conv=notrunc 2>&1 | tail -1" | Out-Null
    $big2 = PA "add $SRC/prcli-400m-sync2.bin --title Big400Sync2"
    Sh "rm -f $SRC/prcli-400m-sync2.bin" | Out-Null
    $pkg = "$BK_DIR/pr-sync-big2.zip"
    $x2 = PA "sync-export $pkg --full"
    $d += "A 加入第二份 400MB → $(Brief $big2)；匯出 → $(Brief $x2)"
    $kill = KillDuringImport $pkg
    # 要打斷在「計畫檔寫成之後、寫筆記檔之前」：這時計畫檔還在、筆記檔還是第一版。不是的話是時機沒打中，不是核心的錯
    $mid = Sh "cat '$notesPath'"
    $inWindow = $kill.Killed -and $kill.PlanLeft -and ($mid -match 'A first version') -and ($mid -notmatch 'A second version')
    # 這裡不可以執行 prcli（開啟時會自動把計畫做完）：直接用 adb shell 改檔
    Sh "echo 'edited on the device after the kill' >> '$notesPath'" | Out-Null
    $null = PB 'list'
    $text = Sh "cat '$notesPath'"
    $after = Sh "ls -1A $SB | grep '^.pr-sync-' ; echo END"
    $dir = $notesPath.Substring(0, $notesPath.LastIndexOf('/'))
    # 只認這一篇的衝突副本（notes/ 裡還有項目 22 留下的 SyncTwo 的副本）
    $copyName = @((Sh "ls -1 '$dir' | grep -F '$stem (conflict'") -split "`n" | Where-Object { $_ }) | Select-Object -First 1
    $copyText = if ($copyName) { Sh "cat '$dir/$copyName'" } else { '' }
    Sh "rm -f $pkg" | Out-Null
    $d += "強制結束：$($kill.Out)；打在計畫檔之後、寫筆記檔之前：$inWindow"
    $d += "重開之後的暫存：$after；筆記檔：`n$text`n這一篇的衝突副本：$copyName`n$copyText"
    $pass = -not (IsErr $x2) -and $inWindow -and ($after -notmatch '\.pr-sync-') `
        -and ($text -match 'edited on the device after the kill') -and ($text -match 'A first version') -and ($text -notmatch 'A second version') `
        -and ($null -ne $copyName) -and ($copyText -match "paperreader_conflict_of: $q") -and ($copyText -match 'A second version')
    @{ Pass = $pass; Detail = $d }
}

# ---------- 項目 25----------
Item 25 'sync-export 一份含 53MB 那篇的完整同步包（記秒數）；來回各一次之後，A 再匯出只帶變更的（記秒數），大小遠小於完整的' {
    $d = @()
    $a53 = PA "add $SRC/big53.pdf --title Big53Sync"
    $full = "$BK_DIR/pr-sync-53full.zip"
    $t1 = TimedD $SA $DEVA "sync-export $full --full"
    $fullSize = SizeOf $full
    $d += "A 加入 53MB → $(Brief $a53)；完整的匯出 → $($t1.Raw -replace "`n", ' ')"
    $i1 = PB "sync-import $full --defaults"
    Sh "rm -f $full" | Out-Null
    # B 的起算點是項目 21 的那一份（A 匯入過），之後 A 加的三份大檔都晚於它，所以這一份也帶著它們
    $eb = "$BK_DIR/pr-sync-53b.zip"
    $xb = PB "sync-export $eb"
    $ia = PA "sync-import $eb --defaults"
    Sh "rm -f $eb" | Out-Null
    $d += "B 匯入 → $(if (IsErr $i1) { Brief $i1 } else { 'OK' })；B 匯出 → mode=$($xb.mode) since=$($xb.since) bytes=$($xb.bytes)；A 匯入 → $(if (IsErr $ia) { Brief $ia } else { 'OK' })"
    $null = PA "thoughts $($ids['big']) 'small change'"
    $chg = "$BK_DIR/pr-sync-53changes.zip"
    $t2 = TimedD $SA $DEVA "sync-export $chg"
    $chgSize = SizeOf $chg
    $d += "完整：$fullSize 位元組，$($t1.Ms) ms；只帶變更（mode=$($t2.Json.mode) since=$($t2.Json.since)）：$chgSize 位元組，$($t2.Ms) ms（秒數只記錄）"
    $pass = ($t1.Json.mode -eq 'full') -and ($fullSize -gt 0) -and -not ((IsErr $i1) -or (IsErr $xb) -or (IsErr $ia)) `
        -and ($t2.Json.mode -eq 'changes') -and ($chgSize -gt 0) -and ($chgSize * 10 -lt $fullSize)
    @{ Pass = $pass; Detail = $d }
}
Sh "rm -rf $SA $SB $BK_DIR/pr-sync*.zip $DEVA $DEVB $SRC/prcli-400m-sync*.bin" | Out-Null

# ===================================================================================================
# 以下是整批匯入的模擬器項目，順延成 26。
# 另外會動 /sdcard/Documents/PaperReaderImport/ 與 $SRC/import/，做完就刪。
# ===================================================================================================
$IR = "$BK_DIR/PaperReaderImport"
$IS = "$SRC/import"

# ---------- 項目 26----------
Item 26 'prcli import 一份五篇的清單（一篇已有、一篇沒有原文、一篇欄位錯）：一份 JSON，回報與檔案樹符合預期；重跑全部略過' {
    $d = @()
    # 三份不同內容的原文（開頭換成 %PDF-）、一份 Markdown 譯本
    $mk = 'dd if=/dev/urandom of={0} bs=1k count=64 2>/dev/null; printf ''%%PDF-'' | dd of={0} conv=notrunc 2>/dev/null'
    Sh ("rm -rf $IR $IS; mkdir -p $IS/L1 $IS/L2 $IS/L3; " + ($mk -f "$IS/L1/a.pdf") + '; ' + ($mk -f "$IS/L2/a.pdf") + '; ' + ($mk -f "$IS/L3/a.pdf") + "; printf '# translation\n' > $IS/L1/zh.md") | Out-Null
    # 第三篇的原文先用「加入」放進文獻庫：匯入時是 already-present
    $pre = PrAt $IR "add $IS/L3/a.pdf --title Already"
    $json = @'
{"format":1,"kind":"paperreader-import","papers":[
 {"csl":{"title":"Import One","author":[{"family":"Zhao"}],"issued":{"date-parts":[[2024]]},"volume":"3"},
  "files":[{"path":"L1/a.pdf","role":"source","form":"pdf"},{"path":"L1/zh.md","role":"translation","form":"markdown"}]},
 {"files":[{"path":"L2/a.pdf","role":"source","form":"pdf"}]},
 {"files":[{"path":"L3/a.pdf","role":"source","form":"pdf"}]},
 {"files":[{"path":"L1/zh.md","role":"translation","form":"markdown"}]},
 {"files":[{"path":"L2/a.pdf","role":"source","form":"pdf","page_map":"sideways"}]}
]}
'@
    $tmp = Join-Path $appDir 'target\prcli-import.json'
    [IO.File]::WriteAllText($tmp, $json)
    & $adb push $tmp "$IS/paperreader-import.json" | Out-Null
    Remove-Item $tmp -ErrorAction SilentlyContinue
    $before = Sh "ls -R $IR/papers"
    $dry = PrAt $IR "import $IS --dry-run"
    $afterDry = Sh "ls -R $IR/papers"
    $run = PrAt $IR "import $IS"
    $again = PrAt $IR "import $IS"
    $tree = Sh "ls -R $IR/papers"
    $d += "先加入的那篇 → $(Brief $pre)"
    $d += "--dry-run → $(Brief $dry)"
    $d += "匯入 → $(Brief $run.report)"
    $d += "重跑 → added=$(@($again.report.added).Count) skipped=$(@($again.report.skipped).Count)"
    $d += "檔案樹：`n$tree"
    $p = $dry.preview
    $skip = @($p.skip | ForEach-Object { "$($_.index):$($_.reason)" }) -join ','
    $added = @($run.report.added | ForEach-Object { $_[0] }) -join ','
    $pass = ($null -eq $dry.report) -and ($before -eq $afterDry) -and ($p.total -eq 5) `
        -and ((@($p.accept | ForEach-Object { $_.index }) -join ',') -eq '0,1') `
        -and ($skip -eq '2:already-present,3:no-source,4:bad-entry') `
        -and ($p.skip[0].detail -eq $pre.id) -and ($p.skip[2].detail -eq 'files[0].page_map') `
        -and ($p.accept[0].notices[0].kind -eq 'bib-fields-dropped') `
        -and ($added -eq '0,1') -and (@($run.report.skipped).Count -eq 3) -and (@($run.report.failed).Count -eq 0) `
        -and ($tree -match 'Zhao 2024 - Import One') -and ($tree -match 'translation\.md') -and ($tree -match 'source\.pdf') `
        -and (@($again.report.added).Count -eq 0) -and (@($again.report.skipped).Count -eq 5)
    @{ Pass = $pass; Detail = $d }
}
Sh "rm -rf $IR $IS" | Out-Null

# ---------- 收尾 ----------
Sh "rm -rf $BK_DIR/pr-backup*.zip $BK_DIR/PaperReaderRestore* $BK_DIR/.pr-export-* $BK_DIR/.pr-restoring-* /data/local/tmp/prcli-timed.out" | Out-Null
Write-Host ''
Write-Host '=== 最後的資料夾（adb shell ls -R）==='
Write-Host (Sh "ls -R $ROOT")
Sh "rm -rf $SRC /data/local/tmp/prcli-bg.out" | Out-Null

Write-Host ''
Write-Host '=== 總結 ==='
$results | Format-Table N, Name, Pass, Seconds -AutoSize | Out-String | Write-Host
$failed = @($results | Where-Object { -not $_.Pass }).Count
Write-Host ("通過 {0} 項，失敗 {1} 項" -f ($results.Count - $failed), $failed)
Stop-Transcript | Out-Null
if ($failed -gt 0) { exit 1 }
