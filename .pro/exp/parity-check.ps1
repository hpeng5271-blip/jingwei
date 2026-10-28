# parity-check.ps1 鈥?8 椤靛叧閿粨鏋?鏁伴噺鏂█锛堝褰撳墠"鑳界敤鐨勬棫澹?璺戯紝闃舵 1 璧勪骇锛?# 鐢ㄦ硶: powershell -File .pro\exp\parity-check.ps1
# 璇存槑: 璧风獥涓€娆★紙WEBVIEW2 remote-debugging锛夛紝鐢?cdp_step.py 鍙?body 鏂囨湰鍚庨€愰〉鏂█锛涘叧绐楀悗鏌ヨ繘绋?0銆?$ErrorActionPreference = "Stop"
$exe = 'D:\projects\jingwei-shell\target\debug\jingwei-gui.exe'
$py = 'D:\projects\jingwei\.pro\exp\cdp_step.py'
$pass = 0
$fail = 0
function Check($name, $ok, $detail) {
  if ($ok) { $script:pass++; "PASS $name  $detail" } else { $script:fail++; "FAIL $name  $detail" }
}
function Step($s) { (python $py $s 2>&1 | Select-Object -First 1) }

Get-Process -Name jingwei-gui -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
$a = Start-Process -FilePath $exe -ArgumentList 'D:\projects\jingwei' -PassThru
Start-Sleep -Seconds 7

$state = Step "state"
$text = Step "text"
Check "list.detail_buttons" ($state -match 'detail=\d+') $state
Check "list.scan_line" ($text -match '133') ""
Check "list.dirs" ($text -match '284') ""

# clean page switched to engine_clean_scan (new kernel). Renders 29 items = A16/B5/C8.
# Must assert BEFORE open: open switches S.root to first list project (not jingwei).
# Poll until scanClean (engine_clean_scan walks global caches, slow) finishes.
Step "tab:clean" | Out-Null
$cleanItems = 0
for ($i = 0; $i -lt 30; $i++) {
  $cleanItems = Step "clean_items"
  if ($cleanItems -match '-> 29$') { break }
  Start-Sleep -Seconds 2
}
Check "clean.items_29" ($cleanItems -match '-> 29$') $cleanItems
$cerr = Step "clean_error"
Check "clean.no_fallback" ($cerr -match '-> null$') $cerr
Step "tab:home" | Out-Null
Start-Sleep -Seconds 2

# open first project
Step "open" | Out-Null
Start-Sleep -Seconds 3
$text2 = Step "text"
Check "project.opened" ($text2 -match 'jingwei') ""

$tabs = @('run','editor','note','graph','machine','clean')
foreach ($t in $tabs) {
  Step ("tab:" + $t) | Out-Null
  Start-Sleep -Seconds 2
  $tx = Step "text"
  Check ("page." + $t) ($tx -match '.') ("len=" + ($tx | Measure-Object -Character).Characters)
}

# machine 页的工具链区块：3b 起改由新内核（engine_env_probe → jw serve）供数。
# 判据不是"有没有这段"，而是**8 个格子**且名字逐项是旧页那 8 个。
Step "tab:machine" | Out-Null
Start-Sleep -Seconds 4
$cnt = Step "envtools"
$names = Step "envtool_names"
Check "machine.env_tools" ($cnt -match '-> 8$') $cnt
$want = @('node','npm','java','go','rustc','gcc','git','uv')
$have = if ($names -match '-> (.*)$') { $Matches[1] } else { '' }
$missing = @($want | Where-Object { $have -notmatch ("(^|,)" + [regex]::Escape($_) + "($|,)") })
Check "machine.env_tool_names" ($missing.Count -eq 0) ("names=" + $have + " missing=" + ($missing -join '|'))

Get-Process -Name jingwei-gui -ErrorAction SilentlyContinue | Select-Object -First 1 | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep -Seconds 3
$after = (Get-Process -Name jingwei-gui -ErrorAction SilentlyContinue | Measure-Object).Count
Check "window.close_zero" ($after -eq 0) ("procs=" + $after)
$jw = (Get-Process -Name jw -ErrorAction SilentlyContinue | Measure-Object).Count
Check "window.close_jw_zero" ($jw -eq 0) ("jw=" + $jw)
Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
""
"RESULT pass=$pass fail=$fail"
if ($fail -gt 0) { exit 1 }