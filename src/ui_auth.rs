//! Web UI 登录：HMAC 签名 session token + 登录页
//!
//! session = base64url(expiry).hmac_sha256(ui_password)
//! cookie: th_ui_session=<token>; HttpOnly; SameSite=Lax; Max-Age=604800 (7天)

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const SESSION_TTL_SECS: i64 = 7 * 24 * 3600;

/// 签发 session token：<b64_expiry>.<b64_sig>
pub fn issue_token(ui_password: &str) -> String {
    let expiry = chrono::Utc::now().timestamp() + SESSION_TTL_SECS;
    let expiry_b64 =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(expiry.to_string().as_bytes());
    let sig = sign(ui_password, &expiry_b64);
    format!("{expiry_b64}.{sig}")
}

/// 校验 Cookie 里的 session token
pub fn check_session(cookie_header: &str, ui_password: &str) -> bool {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("th_ui_session=") {
            let v = v.trim();
            let Some((exp_b64, sig)) = v.split_once('.') else {
                return false;
            };
            // 签名校验（常数时间）
            let expect = sign(ui_password, exp_b64);
            if sig != expect {
                return false;
            }
            // 过期校验
            let Ok(decoded) =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(exp_b64.as_bytes())
            else {
                return false;
            };
            let Ok(exp_str) = String::from_utf8(decoded) else {
                return false;
            };
            let Ok(exp) = exp_str.parse::<i64>() else {
                return false;
            };
            if exp < chrono::Utc::now().timestamp() {
                return false;
            }
            return true;
        }
    }
    false
}

fn sign(ui_password: &str, expiry_b64: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(ui_password.as_bytes()).expect("HMAC accepts any key length");
    mac.update(b"th_ui_session_v1");
    mac.update(expiry_b64.as_bytes());
    let out = mac.finalize().into_bytes();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(out)
}

/// 登录页 HTML（最小暗色风格，与主面板一致）
pub const LOGIN_HTML: &str = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>TokenHarbor2API - 登录</title>
<style>
:root { --bg:#0b0f14; --card:#131a23; --border:#24303d; --text:#e6edf3; --muted:#8b98a5; --accent:#38bdf8; }
* { box-sizing:border-box; margin:0; padding:0; }
body { background:var(--bg); color:var(--text); font-family:-apple-system,'Segoe UI',Roboto,'Microsoft YaHei',sans-serif; min-height:100vh; display:flex; align-items:center; justify-content:center; }
.login-box { background:var(--card); border:1px solid var(--border); border-radius:12px; padding:32px 28px; width:100%; max-width:360px; }
h1 { font-size:18px; margin-bottom:6px; }
p { color:var(--muted); font-size:13px; margin-bottom:20px; }
label { display:block; color:var(--muted); font-size:12px; margin:8px 0 4px; }
input { background:var(--bg); border:1px solid var(--border); color:var(--text); border-radius:6px; padding:10px 12px; font-size:14px; width:100%; }
input:focus { border-color:var(--accent); outline:none; }
button { background:var(--accent); color:#04121a; border:none; border-radius:6px; padding:10px 14px; cursor:pointer; font-size:14px; font-weight:600; width:100%; margin-top:16px; }
button:hover { filter:brightness(1.12); }
#msg { color:#f87171; font-size:13px; margin-top:12px; min-height:18px; }
</style>
</head>
<body>
<div class="login-box">
  <h1>TokenHarbor2API 控制台</h1>
  <p>请输入管理密码以继续</p>
  <label for="pwd">管理密码</label>
  <input type="password" id="pwd" autocomplete="current-password" autofocus>
  <button id="btn">登录</button>
  <div id="msg"></div>
</div>
<script>
const btn = document.getElementById('btn');
const pwd = document.getElementById('pwd');
const msg = document.getElementById('msg');
async function login() {
  const v = pwd.value;
  if (!v) { msg.textContent = '请输入密码'; return; }
  msg.textContent = '';
  btn.disabled = true;
  try {
    const r = await fetch('/api/ui/login', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ password: v }) });
    const d = await r.json().catch(() => ({}));
    if (r.ok) { location.href = '/ui'; }
    else { msg.textContent = d.error?.message || ('登录失败 HTTP ' + r.status); btn.disabled = false; }
  } catch (e) { msg.textContent = '请求失败: ' + e.message; btn.disabled = false; }
}
btn.addEventListener('click', login);
pwd.addEventListener('keydown', e => { if (e.key === 'Enter') login(); });
</script>
</body>
</html>
"##;
