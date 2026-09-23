//! 内置 Web 控制面板（总览 / 凭证 / 模型 / 接入指南）
//!
//! 轻量无构建：单 HTML + 原生 JS + CSS，Rust 直接内嵌。

pub const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>TokenHarbor2API 控制台</title>
<style>
:root {
  --bg:#0b0f14; --card:#131a23; --border:#24303d; --text:#e6edf3; --muted:#8b98a5;
  --accent:#38bdf8; --accent-soft:#0e2a3f; --ok:#34d399; --warn:#fbbf24; --err:#f87171;
}
* { box-sizing:border-box; margin:0; padding:0; }
body { background:var(--bg); color:var(--text); font-family:-apple-system,'Segoe UI',Roboto,'Microsoft YaHei',sans-serif; min-height:100vh; }
@media (prefers-reduced-motion: reduce) { * { animation:none !important; transition:none !important; } }
header { display:flex; align-items:center; justify-content:space-between; padding:14px 24px; border-bottom:1px solid var(--border); background:var(--card); position:sticky; top:0; z-index:10; }
header h1 { font-size:16px; font-weight:600; }
header .dot { display:inline-block; width:8px; height:8px; border-radius:50%; background:var(--muted); margin-right:8px; vertical-align:middle; }
header .dot.ok { background:var(--ok); } header .dot.err { background:var(--err); }
.hstat { display:flex; gap:16px; font-size:12px; color:var(--muted); }
.hstat b { color:var(--text); }
main { max-width:1180px; margin:0 auto; padding:20px 24px 60px; }
nav { display:flex; gap:2px; margin-bottom:20px; border-bottom:1px solid var(--border); flex-wrap:wrap; }
nav button { background:transparent; border:none; color:var(--muted); padding:10px 16px; cursor:pointer; font-size:14px; border-bottom:2px solid transparent; }
nav button.active { color:var(--text); border-bottom-color:var(--accent); }
nav button:hover { color:var(--text); }
button:focus-visible, input:focus-visible, textarea:focus-visible, select:focus-visible, a:focus-visible { outline:2px solid #7dd3fc; outline-offset:2px; }
.sr-only { position:absolute; width:1px; height:1px; padding:0; margin:-1px; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap; border:0; }
.cards { display:grid; grid-template-columns:repeat(auto-fit,minmax(170px,1fr)); gap:14px; margin-bottom:20px; }
.card { background:var(--card); border:1px solid var(--border); border-radius:10px; padding:14px 16px; }
.card .num { font-size:24px; font-weight:700; margin-top:4px; }
.card .lbl { color:var(--muted); font-size:13px; }
.panel { background:var(--card); border:1px solid var(--border); border-radius:10px; padding:16px; margin-bottom:20px; }
.panel h2 { font-size:15px; margin-bottom:12px; color:var(--muted); font-weight:600; }
table { width:100%; border-collapse:collapse; font-size:13px; }
th,td { text-align:left; padding:8px 10px; border-bottom:1px solid var(--border); }
th { color:var(--muted); font-weight:500; }
.badge { display:inline-block; padding:2px 8px; border-radius:20px; font-size:12px; }
.badge.ok { background:#0c3b2e; color:var(--ok); }
.badge.warn { background:#3d2f0a; color:var(--warn); }
.badge.err { background:#3b0f0f; color:var(--err); }
.badge.dim { background:#1c2530; color:var(--muted); }
.chip { display:inline-block; background:#1c2530; border:1px solid var(--border); border-radius:6px; padding:2px 8px; margin:2px; font-size:12px; }
button { background:var(--accent); color:#04121a; border:none; border-radius:6px; padding:6px 14px; cursor:pointer; font-size:13px; font-weight:600; }
button:hover { filter:brightness(1.12); }
button:disabled { opacity:.5; cursor:not-allowed; }
button.ghost { background:transparent; border:1px solid var(--border); color:var(--text); }
button.ghost:hover { border-color:var(--accent); color:var(--accent); }
button.sm { padding:3px 10px; font-size:12px; }
input,textarea,select { background:var(--bg); border:1px solid var(--border); color:var(--text); border-radius:6px; padding:8px 10px; font-size:13px; width:100%; font-family:inherit; }
input:focus,textarea:focus,select:focus { border-color:var(--accent); }
textarea { min-height:80px; resize:vertical; }
label { display:block; color:var(--muted); font-size:12px; margin:8px 0 4px; }
.row { display:flex; gap:8px; align-items:center; flex-wrap:wrap; }
.empty { color:var(--muted); font-size:13px; padding:12px 4px; }
#toast { position:fixed; bottom:24px; left:50%; transform:translateX(-50%); background:var(--card); border:1px solid var(--accent); padding:10px 20px; border-radius:10px; display:none; z-index:100; font-size:13px; box-shadow:0 8px 24px rgba(0,0,0,.5); }
.guide-box { background:var(--bg); border:1px solid var(--border); border-radius:8px; padding:10px 12px; font-family:ui-monospace,Consolas,monospace; font-size:12px; white-space:pre-wrap; word-break:break-all; }
kbd { background:#1c2530; border:1px solid var(--border); border-radius:4px; padding:1px 6px; font-size:12px; font-family:ui-monospace,Consolas,monospace; }
</style>
</head>
<body>
<header>
  <h1><span class="dot ok" id="hdot"></span>TokenHarbor2API 控制台</h1>
  <div class="hstat"><span>模型 <b id="hmodels">-</b></span><span>凭证 <b id="hcreds">-</b></span><span>版本 <b>0.1.0</b></span></div>
</header>
<main>
<nav>
  <button data-tab="overview" class="active">总览</button>
  <button data-tab="tokens">凭证</button>
  <button data-tab="models">模型</button>
  <button data-tab="guide">接入指南</button>
</nav>

<!-- 总览 -->
<section id="tab-overview">
  <div class="cards" id="ov-cards"></div>
  <div class="panel"><h2>立即开始</h2>
    <div class="guide-box" id="ov-guide"></div>
  </div>
</section>

<!-- 凭证 -->
<section id="tab-tokens" style="display:none">
  <div class="panel"><h2>导入 TokenHarbor Cookie</h2>
    <p class="empty">在浏览器登录 tokenharbor.ai → F12 → Network → 请求头里复制完整 <kbd>cookie:</kbd> 整行（含 <kbd>sb-auth-auth-token.0</kbd> / <kbd>sb-auth-auth-token.1</kbd> / <kbd>th_sid</kbd>）粘贴到下面。</p>
    <textarea id="cookie-input" placeholder="sb-auth-auth-token.0=...; sb-auth-auth-token.1=...; th_sid=..."></textarea>
    <div class="row" style="margin-top:10px"><button onclick="importCookie()">导入凭证</button></div>
  </div>
  <div class="panel"><h2>凭证列表</h2><table><thead><tr><th>ID</th><th>标签</th><th>Cookie（掩码）</th><th>健康分</th><th>失败</th><th>操作</th></tr></thead><tbody id="tokens-tbody"></tbody></table></div>
</section>

<!-- 模型 -->
<section id="tab-models" style="display:none">
  <div class="panel"><table><thead><tr><th>模型 ID</th><th>名称</th><th>家族</th><th>档位</th><th>价格 $/1M</th><th>输入模态</th><th>输出</th><th>能力</th></tr></thead><tbody id="models-tbody"></tbody></table></div>
</section>

<!-- 接入指南 -->
<section id="tab-guide" style="display:none">
  <div class="panel"><h2>Claude Code</h2>
    <div class="guide-box" id="g-claude"></div>
  </div>
  <div class="panel"><h2>OpenAI 兼容客户端（Cursor / LobeChat / SDK）</h2>
    <div class="guide-box" id="g-openai"></div>
  </div>
</section>
</main>
<div id="toast"></div>
<script>
const $ = s => document.querySelector(s);
const tabs = document.querySelectorAll('nav button');
tabs.forEach(b => b.addEventListener('click', () => {
  tabs.forEach(x => x.classList.remove('active'));
  b.classList.add('active');
  document.querySelectorAll('main > section').forEach(s => s.style.display = 'none');
  $('#tab-' + b.dataset.tab).style.display = '';
  if (b.dataset.tab === 'tokens') loadTokens();
  if (b.dataset.tab === 'models') loadModels();
  if (b.dataset.tab === 'guide') loadGuide();
  if (b.dataset.tab === 'overview') loadOverview();
}));
function toast(msg) { const t = $('#toast'); t.textContent = msg; t.style.display = 'block'; setTimeout(() => t.style.display = 'none', 2600); }
const API = async (url, opts = {}) => {
  const key = localStorage.getItem('th_key');
  const headers = { 'content-type': 'application/json', ...(opts.headers || {}) };
  if (key) headers['authorization'] = 'Bearer ' + key;
  const r = await fetch(url, { ...opts, headers });
  const ct = r.headers.get('content-type') || '';
  const data = ct.includes('json') ? await r.json().catch(() => ({})) : await r.text().catch(() => '');
  if (!r.ok) throw new Error(data.error?.message || data.detail || ('HTTP ' + r.status));
  return data;
};
async function loadOverview() {
  try {
    const health = await API('/healthz');
    $('#hdot').className = 'dot ok'; $('#hdot').title = '网关正常';
    $('#hmodels').textContent = health.models; $('#hcreds').textContent = health.credentials;
    $('#ov-cards').innerHTML =
      `<div class="card"><div class="lbl">模型总数</div><div class="num">${health.models}</div></div>` +
      `<div class="card"><div class="lbl">凭证数</div><div class="num">${health.credentials}</div></div>` +
      `<div class="card"><div class="lbl">版本</div><div class="num">0.1.0</div></div>`;
    const guide = await API('/api/guide');
    $('#ov-guide').textContent =
`Base URL : ${guide.base_url}
API Key  : ${guide.api_keys_configured ? '已配置（见“接入指南”/ 面板生成）' : '未配置（本机可随意填写）'}
模型     : ${guide.models.join(', ') || '（免费模型）'}
凭证数   : ${guide.credential_count}（需 ≥1 才能请求）`;
  } catch (e) { $('#hdot').className = 'dot err'; toast('总览加载失败: ' + e.message); }
}
async function loadTokens() {
  try {
    const d = await API('/api/tokens');
    $('#hcreds').textContent = (d.tokens || []).length;
    $('#tokens-tbody').innerHTML = (d.tokens || []).map(t => `<tr>
      <td><kbd>${t.id.slice(0,8)}</kbd></td><td>${esc(t.label)}</td><td><kbd>${esc(t.cookie_masked)}</kbd></td>
      <td>${(t.health*100).toFixed(0)}%</td><td>${t.failures}</td>
      <td class="row"><button class="sm ghost" onclick="checkToken('${t.id}')">检查</button><button class="sm ghost" style="color:var(--err)" onclick="delToken('${t.id}')">删除</button></td></tr>`).join('') || '<tr><td colspan="6" class="empty">还没有凭证</td></tr>';
  } catch (e) { toast('凭证加载失败: ' + e.message); }
}
async function importCookie() {
  const cookie = $('#cookie-input').value.trim();
  if (!cookie) return toast('请先粘贴 Cookie');
  try {
    const d = await API('/api/tokens/import', { method: 'POST', body: JSON.stringify({ cookie }) });
    $('#cookie-input').value = '';
    toast('导入成功: ' + d.id.slice(0,8));
    loadTokens(); loadOverview();
  } catch (e) { toast('导入失败: ' + e.message); }
}
async function checkToken(id) {
  try { await API('/api/tokens/check', { method: 'POST', body: JSON.stringify({ id }) }); toast('凭证有效'); loadTokens(); }
  catch (e) { toast('检查失败: ' + e.message); loadTokens(); }
}
async function delToken(id) {
  if (!confirm('删除该凭证？')) return;
  try { await API('/api/tokens/delete', { method: 'POST', body: JSON.stringify({ id }) }); toast('已删除'); loadTokens(); loadOverview(); }
  catch (e) { toast('删除失败: ' + e.message); }
}
async function loadModels() {
  try {
    const r = await fetch('/v1/models'); const d = await r.json();
    $('#hmodels').textContent = (d.data || []).length;
    const extra = await fetch('/api/guide').then(r => r.json()).catch(() => ({ models: [] }));
    $('#models-tbody').innerHTML = (d.data || []).map(m => `<tr>
      <td><kbd>${esc(m.id)}</kbd></td><td>${esc(m.id.split('/').pop())}</td><td>${esc(m.owned_by)}</td>
      <td>${m.id.includes(':free') ? '<span class="badge ok">free</span>' : '<span class="badge dim">paid</span>'}</td>
      <td>${m.id.includes(':free') ? '0' : '见面板'}</td><td>-</td><td>-</td><td>-</td></tr>`).join('') || '<tr><td colspan="8" class="empty">无模型</td></tr>';
  } catch (e) { toast('模型加载失败: ' + e.message); }
}
function loadGuide() {
  const port = location.port || '47830';
  $('#g-claude').textContent =
`# Claude Code
export ANTHROPIC_BASE_URL=http://127.0.0.1:${port}
export ANTHROPIC_API_KEY=sk-local   # 面板生成 Key 后替换

模型：/v1/models 中任选（免费建议 deepseek-v4.1-flash:free）`;
  $('#g-openai').textContent =
`# Cursor / LobeChat / NextChat / OpenAI SDK
Base URL: http://127.0.0.1:${port}/v1
API Key : sk-local（或面板生成）
模型    : 从 /v1/models 选择，免费模型带 :free 后缀

# Python
from openai import OpenAI
client = OpenAI(base_url="http://127.0.0.1:${port}/v1", api_key="sk-local")
resp = client.chat.completions.create(
    model="deepseek-v4.1-flash:free",
    messages=[{"role":"user","content":"你好"}],
    stream=True,
)
for chunk in resp: print(chunk.choices[0].delta.content, end="")`;
}
function esc(s) { return String(s).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }
loadOverview();
</script>
</body>
</html>
"##;
