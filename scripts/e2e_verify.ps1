# TokenHarbor2API 真实 E2E 验证脚本
# 前置：网关运行 + 已导入有效 Cookie（/api/tokens/login 或 import）
$base = "http://127.0.0.1:47830"
$pass = 0; $fail = 0
function Check($name, $cond) {
  if ($cond) { $script:pass++; Write-Host "  [PASS] $name" -ForegroundColor Green }
  else { $script:fail++; Write-Host "  [FAIL] $name" -ForegroundColor Red }
}

Write-Host "=== 1. 健康检查 ===" -ForegroundColor Cyan
$h = Invoke-RestMethod "$base/healthz" -TimeoutSec 5
Check "healthz models>=20" ($h.models -ge 20)

Write-Host "=== 2. 模型列表 ===" -ForegroundColor Cyan
$m = Invoke-RestMethod "$base/v1/models" -TimeoutSec 5
Check "models count 23" ($m.data.Count -eq 23)
Check "has th-rudder:free" (($m.data.id) -contains "th-rudder:free")

Write-Host "=== 3. OpenAI 流式对话 ===" -ForegroundColor Cyan
$body = '{"model":"deepseek-v4.1-flash:free","messages":[{"role":"user","content":"用一句话介绍你自己"}],"stream":true}'
$out = curl.exe --noproxy "*" -s -m 60 -X POST "$base/v1/chat/completions" -H "Content-Type: application/json" -d $body
Check "OpenAI stream has content" ($out -match '"content":"[^"]+[^"]"')
Check "OpenAI stream [DONE]" ($out -match "\[DONE\]")

Write-Host "=== 4. Anthropic 流式对话 ===" -ForegroundColor Cyan
$body2 = '{"model":"deepseek-v4.1-flash:free","messages":[{"role":"user","content":"2+3等于几？只回数字"}],"max_tokens":100,"stream":true}'
$out2 = curl.exe --noproxy "*" -s -m 60 -X POST "$base/v1/messages" -H "Content-Type: application/json" -H "x-api-key: sk-local" -d $body2
Check "Anthropic message_start" ($out2 -match "message_start")
Check "Anthropic content_block" ($out2 -match "content_block_delta")

Write-Host "=== 5. 非流式 + 多轮上下文 ===" -ForegroundColor Cyan
$t1 = curl.exe --noproxy "*" -s -m 60 -X POST "$base/v1/chat/completions" -H "Content-Type: application/json" -d '{"model":"deepseek-v4.1-flash:free","messages":[{"role":"user","content":"记住我的名字叫小海鸥"}]}'
$t2 = curl.exe --noproxy "*" -s -m 60 -X POST "$base/v1/chat/completions" -H "Content-Type: application/json" -d '{"model":"deepseek-v4.1-flash:free","messages":[{"role":"user","content":"我叫什么名字？只回名字"}]}'
Check "T2 recalls name" ($t2 -match "小海鸥")

Write-Host "=== 6. 凭证续期 ===" -ForegroundColor Cyan
$r = curl.exe --noproxy "*" -s -m 30 -X POST "$base/api/tokens/refresh-all"
Check "refresh-all returns ok" ($r -match '"ok":true')

Write-Host ""
Write-Host "E2E RESULT: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Red" })
exit $fail
