# TokenHarbor2API 高并发压测（需已导入凭证）
# 用法: .\scripts\concurrency_test.ps1 [-N 20]
param([int]$N = 20)
$base = "http://127.0.0.1:47830"
$body = '{"model":"deepseek-v4.1-flash:free","messages":[{"role":"user","content":"1+1=? 只回数字"}],"stream":true}'
Write-Host "并发压测: $N 个并发请求（DeepSeek 免费）" -ForegroundColor Cyan
$jobs = @()
for ($i=0; $i -lt $N; $i++) {
  $jobs += Start-Job -ScriptBlock {
    param($url, $b)
    $r = curl.exe --noproxy "*" -s -m 60 -X POST "$url/v1/chat/completions" -H "Content-Type: application/json" -d $b
    $code = curl.exe --noproxy "*" -s -o NUL -w "%{http_code}" -X POST "$url/v1/chat/completions" -H "Content-Type: application/json" -d $b
    "$code|$($r.Substring(0, [Math]::Min(80, $r.Length)))"
  } -ArgumentList $base, $body
}
$results = $jobs | Wait-Job -Timeout 120 | Receive-Job
$jobs | Remove-Job -Force
$ok = ($results | Where-Object { $_ -match "^200" }).Count
$fail = $results.Count - $ok
Write-Host "结果: $ok 成功 / $fail 失败（共 $($results.Count)）" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Red" })
Write-Host "采样:"; $results | Select-Object -First 5
exit $(if ($fail -eq 0) { 0 } else { 1 })
