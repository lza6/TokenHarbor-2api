# TokenHarbor2API 高并发压测（需已导入凭证）
param([int]$N = 20)
$base = "http://127.0.0.1:47830"
$body = '{"model":"deepseek-v4.1-flash:free","messages":[{"role":"user","content":"1+1=? 只回数字"}],"stream":true}'
Write-Host "并发压测: $N 个并发请求（DeepSeek 免费）" -ForegroundColor Cyan

$script:results = [System.Collections.Concurrent.ConcurrentBag[string]]::new()
$script:lock = [System.Threading.Mutex]::new()

1..$N | ForEach-Object {
  Start-ThreadJob -ScriptBlock {
    param($url, $b, $bag, $lk)
    try {
      $r = curl.exe --noproxy "*" -s -m 60 -w "HTTPCODE:%{http_code}" -X POST "$url/v1/chat/completions" -H "Content-Type: application/json" -d $b
      $bag.Add($r)
    } catch {
      $bag.Add("EXCEPTION:" + $_.Exception.Message)
    }
  } -ArgumentList $base, $body, $script:results, $script:lock
} | Out-Null

# 等所有线程任务完成
Start-Sleep -Seconds 90
$ok = @($results | Where-Object { $_ -match "HTTPCODE:200" }).Count
$err = @($results | Where-Object { $_ -notmatch "HTTPCODE:200" }).Count
Write-Host "结果: $ok 成功 / $err 失败（共 $($results.Count)）" -ForegroundColor $(if ($err -eq 0) { "Green" } else { "Red" })
$results | Select-Object -First 6
exit $(if ($err -eq 0) { 0 } else { 1 })
