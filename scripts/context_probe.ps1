param(
  [string]$Base = "http://52.141.3.10:47830",
  [string]$Key = "sk-REPLACE_WITH_YOUR_KEY",
  [int[]]$Sizes = @(25, 250)
)
$ErrorActionPreference = "Continue"
foreach ($k in $Sizes) {
  $chars = $k * 1000 * 4
  $word = "token "
  $reps = [int]($chars / $word.Length)
  $content = ($word * $reps)
  $body = @{ model = "th-rudder:free"; messages = @(@{ role = "user"; content = $content + " now reply just OK" }); max_tokens = 16 } | ConvertTo-Json -Depth 6 -Compress
  $tmp = Join-Path $env:TEMP "th_probe_$k.json"
  [System.IO.File]::WriteAllText($tmp, $body, (New-Object System.Text.UTF8Encoding($false)))
  Write-Host "== size ${k}k tokens (payload KB=$([math]::Round((Get-Item $tmp).Length/1KB,0))) ==" -ForegroundColor Cyan
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $respFile = Join-Path $env:TEMP "th_probe_${k}_resp.txt"
  $code = curl.exe --noproxy "*" -s -o $respFile -w "%{http_code}" -m 600 -X POST "$Base/v1/chat/completions" -H "Content-Type: application/json" -H "Authorization: Bearer $Key" --data-binary "@$tmp"
  $sw.Stop()
  Write-Host "HTTP $code in $($sw.Elapsed.TotalSeconds.ToString('F1'))s"
  $head = Get-Content $respFile -Raw -ErrorAction SilentlyContinue
  if ($head) { $head = $head.Substring(0, [Math]::Min(400, $head.Length)) }
  Write-Host "resp head: $head" -ForegroundColor Yellow
  Remove-Item $tmp, $respFile -ErrorAction SilentlyContinue
}