//! 日志/错误脱敏：API Key、Cookie、Bearer Token 等敏感值统一打码
//! 同时提供截断，避免上游错误原文透传给客户端。

/// 把字符串中疑似敏感值打码（幂等）
pub fn redact(s: &str) -> String {
    let mut out = s.to_string();
    // 1) sk- API Key（sk-th-xxx / sk-xxx）
    let re1 = regex::Regex::new(r#"(?i)\bsk-(th-)?[A-Za-z0-9]{8,}\b"#).unwrap();
    out = re1.replace_all(&out, "sk-***").into_owned();
    // 2) Bearer / x-api-key
    let re2 = regex::Regex::new(r#"(?i)(Bearer|x-api-key)\s+[A-Za-z0-9._~+/=-]{12,}"#).unwrap();
    out = re2.replace_all(&out, "$1 ***").into_owned();
    // 3) Supabase cookie 值（sb-auth-auth-token.0/1=base64-...）
    let re3 = regex::Regex::new(r#"(?i)sb-auth-auth-token\.[01]=[^;\s"]{12,}"#).unwrap();
    out = re3
        .replace_all(&out, "sb-auth-auth-token.*=***")
        .into_owned();
    // 4) 任意超长 base64（≥40 且含 -_ 或 /+，疑似 token）
    let re4 = regex::Regex::new(r#"[A-Za-z0-9+/_-]{40,}"#).unwrap();
    out = re4.replace_all(&out, "***").into_owned();
    // 5) JSON 里的 password 字段值
    let re5 = regex::Regex::new(r#"(?i)("password"\s*:\s*")[^"]+"#).unwrap();
    out = re5.replace_all(&out, "$1***").into_owned();
    out
}

/// 截断到 n 字符（按 char，避免切断 UTF-8）
pub fn truncate(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        s.to_string()
    } else {
        // 截断到 n 字符并补省略号（n>=1）
        let cut: String = chars[..n.saturating_sub(1)].iter().collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_masks_api_key_and_cookie() {
        let s = "Bearer sk-th-0123456789abcdef0123456789abcdef cookie=sb-auth-auth-token.0=base64-eyJmb28iOiJiYXIifQ";
        let r = redact(s);
        assert!(!r.contains("0123456789abcdef0123456789abcdef"));
        assert!(!r.contains("base64-eyJmb28i"));
        assert!(r.contains("sk-***"));
        assert!(r.contains("sb-auth-auth-token.*=***"));
    }

    #[test]
    fn truncate_keeps_utf8() {
        let s = "你好世界你好世界";
        assert_eq!(truncate(s, 4), "你好世…"); // 3 字符 + 省略号，总长 4
        assert_eq!(truncate(s, 5), "你好世界…");
        assert!(truncate(s, 100).ends_with("世界"));
    }
}
