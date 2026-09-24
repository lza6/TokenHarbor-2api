//! 凭证导入解析器：自动识别多种格式并提取 Cookie
//!
//! 支持的输入形态（自动检测，无需用户手动切换）：
//! 1. 裸 Cookie 头：`name=value; name2=value2`
//! 2. `Cookie: ...` 前缀
//! 3. curl `-b` / `--cookie` 参数：`curl ... -b "a=1; b=2"` 或 Windows 转义 `-b ^"a=1^"`
//! 4. curl `-H "Cookie: ..."` 参数
//! 5. HAR 文件（JSON）：从 entries[].request.cookies 提取
//! 6. Netscape cookie jar（#HttpOnly_ 前缀行）
//! 7. Chromium/Firefox JSON cookie 导出（[{name,value,domain,...}]）

use serde_json::Value;

/// 从任意输入提取 Cookie 字符串（name=value; ...）
pub fn extract_cookie(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    // 1. 尝试 JSON 形态（HAR / Chromium / Firefox 导出）
    if let Some(c) = try_har_or_json(raw) {
        if !c.is_empty() {
            return Some(c);
        }
    }

    // 2. 尝试 Netscape cookie jar（行格式）
    if let Some(c) = try_netscape(raw) {
        if !c.is_empty() {
            return Some(c);
        }
    }

    // 3. curl 命令形态（-b / -H 等）
    if let Some(c) = try_curl_command(raw) {
        if !c.is_empty() {
            return Some(c);
        }
    }

    // 4. 普通 Cookie 头形态
    Some(normalize_cookie_header(raw))
}

/// 从 HAR / Chromium / Firefox JSON 提取 cookie
fn try_har_or_json(raw: &str) -> Option<String> {
    if !raw.trim_start().starts_with('{') && !raw.trim_start().starts_with('[') {
        return None;
    }
    let parsed: Value = serde_json::from_str(raw).ok()?;

    // HAR: {"log":{"entries":[{"request":{"cookies":[{name,value}]}}]}}
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(entries) = parsed
        .get("log")
        .and_then(|l| l.get("entries"))
        .and_then(|e| e.as_array())
    {
        for entry in entries {
            if let Some(cookies) = entry
                .get("request")
                .and_then(|r| r.get("cookies"))
                .and_then(|c| c.as_array())
            {
                for ck in cookies {
                    if let (Some(n), Some(v)) = (
                        ck.get("name").and_then(|v| v.as_str()),
                        ck.get("value").and_then(|v| v.as_str()),
                    ) {
                        pairs.push((n.to_string(), v.to_string()));
                    }
                }
            }
        }
    } else if let Some(arr) = parsed.as_array() {
        // Chromium/Firefox: [{name,value,domain,expirationDate...}]
        for ck in arr {
            if let (Some(n), Some(v)) = (
                ck.get("name").and_then(|v| v.as_str()),
                ck.get("value").and_then(|v| v.as_str()),
            ) {
                pairs.push((n.to_string(), v.to_string()));
            }
        }
    }
    if pairs.is_empty() {
        return None;
    }
    Some(join_pairs(pairs))
}

/// Netscape cookie jar：#HttpOnly_tokenharbor.ai TRUE / FALSE name value ...
fn try_netscape(raw: &str) -> Option<String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        // 跳过纯注释行（#HttpOnly_ 是合法域名前缀，不是注释）
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_")) {
            continue;
        }
        // 格式：domain flag path secure expiry name value
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 7 {
            let name = cols[5].trim();
            let value = cols[6].trim();
            if !name.is_empty() {
                pairs.push((name.to_string(), value.to_string()));
            }
        }
    }
    if pairs.is_empty() {
        None
    } else {
        Some(join_pairs(pairs))
    }
}

/// curl 命令形态：`curl 'https://...' -H 'Cookie: a=1; b=2'` 或 `-b "a=1; b=2"`
fn try_curl_command(raw: &str) -> Option<String> {
    // 先去掉 Windows cmd 的 ^ 转义（^" -> "，^% -> % 等）
    let clean = raw
        .replace("^\"", "\"")
        .replace("^'", "'")
        .replace("^%", "%");
    // 如果是整行命令（含 curl 关键字），用引号解析
    if !clean.contains("curl")
        && !clean.contains("Cookie:")
        && !clean.contains("-b ")
        && !clean.contains("-H ")
    {
        return None;
    }
    // 提取所有带引号的片段
    let mut quoted: Vec<String> = Vec::new();
    let bytes = clean.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            let quote = b;
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != quote {
                end += 1;
            }
            if end < bytes.len() {
                quoted.push(clean[start..end].to_string());
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    // 优先找含 "Cookie:" 或 "=" 的片段
    for q in &quoted {
        let t = q.trim();
        if t.contains("Cookie:") {
            return Some(normalize_cookie_header(t));
        }
    }
    for q in &quoted {
        let t = q.trim();
        if t.contains('=') && (t.contains(';') || t.contains("sb-auth")) {
            return Some(normalize_cookie_header(t));
        }
    }
    None
}

/// 普通 Cookie 头规范化：去掉 Cookie: 前缀 / ^ 转义 / 引号，保留 name=value 对
fn normalize_cookie_header(raw: &str) -> String {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .trim_start_matches("Cookie:")
        .trim()
        .trim_matches('^')
        .trim_matches('"')
        .trim();
    // 如果整行是 curl 命令但没被上面抓到，这里保守提取
    let candidate = trimmed
        .split(';')
        .map(|p| p.trim())
        .filter(|p| p.contains('=') && !p.starts_with("curl ") && !p.contains(" --"))
        .filter_map(|p| {
            // 去掉值里的引号
            let mut parts = p.splitn(2, '=');
            let k = parts
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
            let v = parts
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
            if k.is_empty() {
                return None;
            }
            Some(format!("{k}={v}"))
        })
        .collect::<Vec<_>>();
    candidate.join("; ")
}

fn join_pairs(pairs: Vec<(String, String)>) -> String {
    pairs
        .iter()
        .filter(|(n, _)| !n.is_empty())
        .map(|(n, v)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_cookie_header() {
        let out = extract_cookie("sb-auth-auth-token.0=aaa; sb-auth-auth-token.1=bbb; th_sid=ccc")
            .unwrap();
        assert!(out.contains("sb-auth-auth-token.0=aaa"));
        assert!(out.contains("th_sid=ccc"));
    }

    #[test]
    fn cookie_prefix() {
        let out = extract_cookie("Cookie: sb-auth-auth-token.0=aaa; th_sid=ccc").unwrap();
        assert!(out.contains("sb-auth-auth-token.0=aaa"));
    }

    #[test]
    fn curl_b_command() {
        let out = extract_cookie(
            "curl 'https://tokenharbor.ai/api/x' -b 'sb-auth-auth-token.0=aaa; th_sid=ccc'",
        )
        .unwrap();
        assert!(out.contains("sb-auth-auth-token.0=aaa"));
        assert!(out.contains("th_sid=ccc"));
    }

    #[test]
    fn curl_h_command() {
        let out = extract_cookie(
            "curl https://tokenharbor.ai -H 'Cookie: sb-auth-auth-token.0=aaa; th_sid=ccc'",
        )
        .unwrap();
        assert!(out.contains("sb-auth-auth-token.0=aaa"));
    }

    #[test]
    fn curl_windows_escaped() {
        let out = extract_cookie("curl -b ^\"sb-auth-auth-token.0=aaa; th_sid=ccc^\"").unwrap();
        assert!(out.contains("sb-auth-auth-token.0=aaa"));
    }

    #[test]
    fn har_file() {
        let har = r#"{"log":{"entries":[{"request":{"cookies":[{"name":"sb-auth-auth-token.0","value":"aaa"},{"name":"th_sid","value":"ccc"}]}}]}}"#;
        let out = extract_cookie(har).unwrap();
        assert!(out.contains("sb-auth-auth-token.0=aaa"));
        assert!(out.contains("th_sid=ccc"));
    }

    #[test]
    fn netscape_jar() {
        let jar = "# Netscape HTTP Cookie File\n#HttpOnly_tokenharbor.ai\tTRUE\t/\tTRUE\t1790224216\tsb-auth-auth-token.0\taaa\n.tokenharbor.ai\tTRUE\t/\tTRUE\t1790224216\tth_sid\tccc\n";
        let out = extract_cookie(jar).unwrap();
        assert!(out.contains("sb-auth-auth-token.0=aaa"));
        assert!(out.contains("th_sid=ccc"));
    }

    #[test]
    fn chromium_json() {
        let json = r#"[{"domain":".tokenharbor.ai","name":"sb-auth-auth-token.0","value":"aaa"},{"domain":".tokenharbor.ai","name":"th_sid","value":"ccc"}]"#;
        let out = extract_cookie(json).unwrap();
        assert!(out.contains("sb-auth-auth-token.0=aaa"));
    }
}
