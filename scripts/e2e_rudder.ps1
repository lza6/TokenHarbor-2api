param([string]$Base = "http://52.141.3.10:47830", [string]$Key = "sk-REPLACE_WITH_YOUR_KEY")
$ErrorActionPreference = "Continue"
function W($t){ Write-Host $t -ForegroundColor Cyan }
function Curl-Json([string]$Path, [string]$Url, [int]$Timeout = 120) {
  $respFile = Join-Path $env:TEMP "th_e2e_resp.txt"
  $code = curl.exe --noproxy "*" -s -o $respFile -w "%{http_code}" -m $Timeout -X POST "$Url" -H "Content-Type: application/json" -H "Authorization: Bearer $Key" --data-binary "@$Path"
  $txt = Get-Content $respFile -Raw -ErrorAction SilentlyContinue
  return @($code, $txt)
}
function Mk([string]$Json) {
  $tmp = Join-Path $env:TEMP "th_e2e_body.json"
  [System.IO.File]::WriteAllText($tmp, $Json, (New-Object System.Text.UTF8Encoding($false)))
  return $tmp
}
W "== 1 chat/completions streaming =="
$b = Mk '{"model":"th-rudder:free","messages":[{"role":"user","content":"hi"}],"stream":true}'
$r = Curl-Json $b "$Base/v1/chat/completions" 120
Write-Output ("  HTTP $($r[0]) has [DONE]: " + ($r[1] -match "\[DONE\]") + " has content: " + ($r[1] -match '"content":"[^"]'))
W "== 2 responses API raw =="
$b2 = Mk '{"model":"th-rudder:free","input":"2+2=?","stream":true}'
$r2 = Curl-Json $b2 "$Base/v1/responses" 120
Write-Output ("  HTTP $($r2[0]) resp head: " + $r2[1].Substring(0,[Math]::Min(300,$r2[1].Length)))
W "== 3 multi-turn memory =="
$b3 = Mk '{"model":"th-rudder:free","messages":[{"role":"user","content":"记住暗号 蓝色海鸥"}]}'
$r3 = Curl-Json $b3 "$Base/v1/chat/completions" 120
Write-Output ("  t1: HTTP $($r3[0]) " + $r3[1].Substring(0,[Math]::Min(160,$r3[1].Length)))
$b4 = Mk '{"model":"th-rudder:free","messages":[{"role":"user","content":"暗号是什么？只回暗号"}]}'
$r4 = Curl-Json $b4 "$Base/v1/chat/completions" 120
Write-Output ("  t2: HTTP $($r4[0]) " + $r4[1].Substring(0,[Math]::Min(220,$r4[1].Length)))
W "== 4 tools / function calling =="
$b5 = Mk '{"model":"th-rudder:free","messages":[{"role":"user","content":"北京天气如何？有工具就用"}],"tools":[{"type":"function","function":{"name":"get_weather","description":"查天气","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}],"tool_choice":"auto"}'
$r5 = Curl-Json $b5 "$Base/v1/chat/completions" 120
Write-Output ("  tools: HTTP $($r5[0]) " + $r5[1].Substring(0,[Math]::Min(400,$r5[1].Length)))