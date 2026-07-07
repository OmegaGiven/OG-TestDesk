//! Pure logic for the Requests tab's `{{name}}` double-brace environment
//! variable substitution: distinct from `request_url.rs`'s single-brace
//! `{path}` variable mechanism (the two intentionally don't overlap — see
//! that module's doc comment and `docs/DESIGN_RICH_REBUILD.md`).
//!
//! Scoping precedence when merging values: local (request-tab) overrides
//! beat the active environment set, which beats global variables.

use std::collections::HashMap;

/// Scans for double-brace `{{name}}` tokens, skipping single-brace `{name}`
/// path-variable spans entirely. Returns distinct names in first-seen order.
pub fn scan_env_variables(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'{' && i + 1 < len && bytes[i + 1] == b'{' {
            match s[i + 2..].find("}}") {
                Some(rel) => {
                    let name = &s[i + 2..i + 2 + rel];
                    if !name.is_empty() && !name.contains('{') && !name.contains('}') && !result.contains(&name.to_string()) {
                        result.push(name.to_string());
                    }
                    i = i + 2 + rel + 2;
                    continue;
                }
                None => break,
            }
        }
        i += 1;
    }
    result
}

/// Merges variable scopes with precedence: `local` > `active_set` > `global`.
pub fn merge_scopes(
    global: &HashMap<String, String>,
    active_set: Option<&HashMap<String, String>>,
    local: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut merged = global.clone();
    if let Some(set) = active_set {
        for (k, v) in set {
            merged.insert(k.clone(), v.clone());
        }
    }
    for (k, v) in local {
        merged.insert(k.clone(), v.clone());
    }
    merged
}

/// Replaces every `{{name}}` token in `s` using `values`, leaving
/// single-brace `{path}` spans untouched. Returns the substituted string
/// plus a list (first-seen order, deduped) of variable names that had no
/// resolved value.
pub fn substitute_env_variables(s: &str, values: &HashMap<String, String>) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut missing = Vec::new();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'{' && i + 1 < len && bytes[i + 1] == b'{' {
            match s[i + 2..].find("}}") {
                Some(rel) => {
                    let name = &s[i + 2..i + 2 + rel];
                    match values.get(name) {
                        Some(v) => out.push_str(v),
                        None => {
                            out.push_str(&s[i..i + 2 + rel + 2]);
                            if !name.is_empty() && !missing.contains(&name.to_string()) {
                                missing.push(name.to_string());
                            }
                        }
                    }
                    i = i + 2 + rel + 2;
                    continue;
                }
                None => {
                    out.push_str(&s[i..]);
                    break;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    (out, missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_env_variables_ignores_single_brace_path_vars() {
        let names = scan_env_variables("{{host}}/users/{id}");
        assert_eq!(names, vec!["host".to_string()]);
    }

    #[test]
    fn scan_env_variables_finds_multiple_deduped_in_order() {
        let names = scan_env_variables("{{host}}/{{token}}/x/{{host}}");
        assert_eq!(names, vec!["host".to_string(), "token".to_string()]);
    }

    #[test]
    fn scan_env_variables_only_path_vars_returns_empty() {
        assert!(scan_env_variables("/a/{id}/b/{slug}").is_empty());
    }

    #[test]
    fn scan_and_path_scanners_do_not_cross_contaminate() {
        let s = "{{base}}/users/{id}?token={{tok}}";
        let env_names = scan_env_variables(s);
        let path_names = crate::request_url::scan_path_variables(s);
        assert_eq!(env_names, vec!["base".to_string(), "tok".to_string()]);
        assert_eq!(path_names, vec!["id".to_string()]);
    }

    #[test]
    fn merge_scopes_precedence_local_beats_set_beats_global() {
        let mut global = HashMap::new();
        global.insert("a".to_string(), "global-a".to_string());
        global.insert("b".to_string(), "global-b".to_string());
        let mut set = HashMap::new();
        set.insert("b".to_string(), "set-b".to_string());
        set.insert("c".to_string(), "set-c".to_string());
        let mut local = HashMap::new();
        local.insert("c".to_string(), "local-c".to_string());

        let merged = merge_scopes(&global, Some(&set), &local);
        assert_eq!(merged.get("a"), Some(&"global-a".to_string()));
        assert_eq!(merged.get("b"), Some(&"set-b".to_string()));
        assert_eq!(merged.get("c"), Some(&"local-c".to_string()));
    }

    #[test]
    fn merge_scopes_no_active_set_falls_back_to_global() {
        let mut global = HashMap::new();
        global.insert("a".to_string(), "global-a".to_string());
        let merged = merge_scopes(&global, None, &HashMap::new());
        assert_eq!(merged.get("a"), Some(&"global-a".to_string()));
    }

    #[test]
    fn substitute_env_variables_replaces_known_and_reports_missing() {
        let mut values = HashMap::new();
        values.insert("host".to_string(), "example.com".to_string());
        let (result, missing) = substitute_env_variables("{{host}}/users/{{token}}", &values);
        assert_eq!(result, "example.com/users/{{token}}");
        assert_eq!(missing, vec!["token".to_string()]);
    }

    #[test]
    fn substitute_env_variables_leaves_path_vars_untouched() {
        let values = HashMap::new();
        let (result, missing) = substitute_env_variables("/users/{id}", &values);
        assert_eq!(result, "/users/{id}");
        assert!(missing.is_empty());
    }

    #[test]
    fn substitute_env_variables_no_missing_when_all_resolved() {
        let mut values = HashMap::new();
        values.insert("a".to_string(), "1".to_string());
        values.insert("b".to_string(), "2".to_string());
        let (result, missing) = substitute_env_variables("{{a}}-{{b}}", &values);
        assert_eq!(result, "1-2");
        assert!(missing.is_empty());
    }
}
