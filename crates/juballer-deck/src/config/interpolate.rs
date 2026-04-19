//! Shell-style variable interpolation for config values.
//! Supports: `$var`, `${var}`, `${var:-default}`.
//! Source of variables: merged (profile.env, process env). Profile env wins.

use std::collections::HashMap;

pub fn interpolate(s: &str, env: &HashMap<String, String>) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b != b'$' {
            out.push(b as char);
            i += 1;
            continue;
        }
        // $
        if i + 1 >= bytes.len() {
            out.push('$');
            break;
        }
        let next = bytes[i + 1];
        if next == b'{' {
            // ${name} or ${name:-default}
            let end = (i + 2..bytes.len()).find(|&j| bytes[j] == b'}');
            if let Some(end) = end {
                let inner = &s[i + 2..end];
                let (name, default) = match inner.find(":-") {
                    Some(p) => (&inner[..p], Some(&inner[p + 2..])),
                    None => (inner, None),
                };
                let v = env
                    .get(name)
                    .cloned()
                    .or_else(|| default.map(|d| d.to_string()))
                    .unwrap_or_default();
                out.push_str(&v);
                i = end + 1;
            } else {
                out.push_str(&s[i..]);
                break;
            }
        } else if next.is_ascii_alphabetic() || next == b'_' {
            let mut end = i + 1;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            let name = &s[i + 1..end];
            let v = env.get(name).cloned().unwrap_or_default();
            out.push_str(&v);
            i = end;
        } else {
            out.push('$');
            i += 1;
        }
    }
    out
}

pub fn build_env(profile_env: &indexmap::IndexMap<String, String>) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = std::env::vars().collect();
    for (k, v) in profile_env {
        m.insert(k.clone(), v.clone());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn bare_variable() {
        let e = env(&[("FOO", "bar")]);
        assert_eq!(interpolate("hello $FOO!", &e), "hello bar!");
    }

    #[test]
    fn braced_variable() {
        let e = env(&[("X", "abc")]);
        assert_eq!(interpolate("pre_${X}_post", &e), "pre_abc_post");
    }

    #[test]
    fn default_when_missing() {
        let e = env(&[]);
        assert_eq!(interpolate("${NOPE:-fallback}", &e), "fallback");
    }

    #[test]
    fn missing_bare_is_empty() {
        let e = env(&[]);
        assert_eq!(interpolate("a${NOPE}b", &e), "ab");
        assert_eq!(interpolate("a$NOPE b", &e), "a b");
    }

    #[test]
    fn literal_dollar() {
        let e = env(&[]);
        assert_eq!(interpolate("price: $5", &e), "price: $5");
    }

    #[test]
    fn unterminated_brace_is_literal() {
        let e = env(&[]);
        assert_eq!(interpolate("${broken", &e), "${broken");
    }
}
