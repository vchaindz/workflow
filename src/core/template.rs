use std::collections::HashMap;

use regex::Regex;

/// Expand template variables in a string.
/// Supports: {{date}}, {{datetime}}, {{hostname}}, {{date_offset Nd}}, custom vars,
/// pipe filters (upper, lower, trim, default, replace, truncate, split, first, last, nth, count),
/// and ternary expressions ({{var | eq "x" ? "yes" : "no"}}).
///
/// Docker format passthrough: expressions starting with `.` or Go template keywords
/// (range, end, if, else, with, define, template, block) pass through unchanged.
pub fn expand_template(input: &str, vars: &HashMap<String, String>) -> String {
    let re = Regex::new(r"\{\{(.*?)\}\}").unwrap();
    let mut result = String::with_capacity(input.len());
    let mut last_end = 0;

    for cap in re.captures_iter(input) {
        let m = cap.get(0).unwrap();
        result.push_str(&input[last_end..m.start()]);

        let inner = cap[1].trim();

        // Docker/Go template passthrough: starts with `.` or is a Go keyword
        if is_docker_passthrough(inner) {
            result.push_str(m.as_str());
            last_end = m.end();
            continue;
        }

        match eval_expression(inner, vars) {
            Some(value) => result.push_str(&value),
            None => result.push_str(m.as_str()), // unknown var, pass through
        }

        last_end = m.end();
    }

    result.push_str(&input[last_end..]);
    result
}

/// Check if an expression should pass through (Docker format strings, Go templates).
fn is_docker_passthrough(inner: &str) -> bool {
    if inner.starts_with('.') {
        return true;
    }
    let first_word = inner.split_whitespace().next().unwrap_or("");
    matches!(
        first_word,
        "range" | "end" | "if" | "else" | "with" | "define" | "template" | "block"
    )
}

/// Evaluate a full template expression including pipes and ternary.
fn eval_expression(expr: &str, vars: &HashMap<String, String>) -> Option<String> {
    // Check for ternary: split on unquoted `?` and `:`
    if let Some(ternary) = parse_ternary(expr) {
        let condition_val = eval_pipe_chain(ternary.condition, vars)?;
        return if condition_val == "true" {
            Some(unquote(ternary.truthy))
        } else {
            Some(unquote(ternary.falsy))
        };
    }

    // Split on unquoted `|` for pipe chain
    let parts = split_pipes(expr);
    eval_pipe_chain_parts(&parts, vars)
}

/// Evaluate a pipe chain given the raw expression (no ternary).
fn eval_pipe_chain(expr: &str, vars: &HashMap<String, String>) -> Option<String> {
    let parts = split_pipes(expr);
    eval_pipe_chain_parts(&parts, vars)
}

/// Evaluate pipe chain from pre-split parts.
fn eval_pipe_chain_parts(parts: &[&str], vars: &HashMap<String, String>) -> Option<String> {
    if parts.is_empty() {
        return None;
    }

    let var_part = parts[0].trim();
    let value = resolve_var(var_part, vars);

    // If no filters and value is None, return None (passthrough)
    if parts.len() == 1 && value.is_none() {
        // Check if it could be a default filter case - no, just pass through
        return None;
    }

    let current = value.unwrap_or_default();
    let mut result = current;

    for &filter_str in &parts[1..] {
        let filter_str = filter_str.trim();
        result = apply_filter(filter_str, &result);
    }

    Some(result)
}

/// Resolve a variable name to its value.
fn resolve_var(name: &str, vars: &HashMap<String, String>) -> Option<String> {
    // Built-in variables
    let now = chrono::Utc::now();

    match name {
        "date" => Some(now.format("%Y-%m-%d").to_string()),
        "datetime" => Some(now.format("%Y-%m-%d_%H-%M-%S").to_string()),
        "hostname" => hostname().ok(),
        _ if name.starts_with("date_offset") => {
            parse_date_offset(name).map(|d| d.format("%Y-%m-%d").to_string())
        }
        _ => vars.get(name).cloned(),
    }
}

/// Parse `date_offset <offset>` where offset is like `-1d`, `7d`, `-2w`, `1w`.
fn parse_date_offset(expr: &str) -> Option<chrono::NaiveDate> {
    let rest = expr.strip_prefix("date_offset")?.trim();
    let today = chrono::Utc::now().date_naive();

    if rest.is_empty() {
        return Some(today);
    }

    let (sign, num_str, unit) = if let Some(stripped) = rest.strip_prefix('-') {
        let (n, u) = split_number_unit(stripped)?;
        (-1i64, n, u)
    } else if let Some(stripped) = rest.strip_prefix('+') {
        let (n, u) = split_number_unit(stripped)?;
        (1i64, n, u)
    } else {
        let (n, u) = split_number_unit(rest)?;
        (1i64, n, u)
    };

    let num: i64 = num_str.parse().ok()?;
    let days = match unit {
        "d" => num * sign,
        "w" => num * 7 * sign,
        _ => return None,
    };

    today.checked_add_signed(chrono::Duration::days(days))
}

fn split_number_unit(s: &str) -> Option<(&str, &str)> {
    let pos = s.find(|c: char| !c.is_ascii_digit())?;
    if pos == 0 {
        return None;
    }
    Some((&s[..pos], &s[pos..]))
}

/// Apply a single filter to a value.
fn apply_filter(filter_str: &str, value: &str) -> String {
    let (name, args) = parse_filter_name_args(filter_str);

    match name {
        "upper" => value.to_uppercase(),
        "lower" => value.to_lowercase(),
        "trim" => value.trim().to_string(),
        "default" => {
            if value.is_empty() {
                args.first().map(|s| s.to_string()).unwrap_or_default()
            } else {
                value.to_string()
            }
        }
        "replace" => {
            if args.len() >= 2 {
                value.replace(args[0], args[1])
            } else {
                value.to_string()
            }
        }
        "truncate" => {
            if let Some(n) = args.first().and_then(|s| s.parse::<usize>().ok()) {
                if value.len() > n {
                    value[..n].to_string()
                } else {
                    value.to_string()
                }
            } else {
                value.to_string()
            }
        }
        "split" => {
            let sep = args.first().map(|s| s.as_ref()).unwrap_or(",");
            value
                .split(sep)
                .collect::<Vec<_>>()
                .join("\n")
        }
        "first" => value.lines().next().unwrap_or("").to_string(),
        "last" => value.lines().last().unwrap_or("").to_string(),
        "nth" => {
            if let Some(n) = args.first().and_then(|s| s.parse::<usize>().ok()) {
                value.lines().nth(n).unwrap_or("").to_string()
            } else {
                value.to_string()
            }
        }
        "count" => value.lines().count().to_string(),
        "eq" => {
            let target = args.first().map(|s| s.as_ref()).unwrap_or("");
            if value == target { "true".to_string() } else { "false".to_string() }
        }
        _ => value.to_string(), // unknown filter, pass through
    }
}

/// Parse a filter into its name and string arguments.
/// E.g. `default "fallback"` -> ("default", ["fallback"])
/// E.g. `replace "old" "new"` -> ("replace", ["old", "new"])
fn parse_filter_name_args(filter_str: &str) -> (&str, Vec<&str>) {
    let filter_str = filter_str.trim();
    let mut parts = Vec::new();

    // Find the filter name (first word)
    let name_end = filter_str
        .find(|c: char| c.is_whitespace())
        .unwrap_or(filter_str.len());
    let name = &filter_str[..name_end];
    let rest = filter_str[name_end..].trim();

    if rest.is_empty() {
        return (name, parts);
    }

    // Parse quoted and unquoted arguments using byte indices
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        if bytes[i] == b'"' {
            i += 1; // consume opening quote
            let start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            parts.push(&rest[start..i]);
            if i < bytes.len() {
                i += 1; // consume closing quote
            }
        } else {
            let start = i;
            while i < bytes.len() && bytes[i] != b' ' {
                i += 1;
            }
            parts.push(&rest[start..i]);
        }
    }

    (name, parts)
}

struct Ternary<'a> {
    condition: &'a str,
    truthy: &'a str,
    falsy: &'a str,
}

/// Parse a ternary expression: `expr | eq "val" ? "yes" : "no"`
fn parse_ternary(expr: &str) -> Option<Ternary<'_>> {
    // Find `?` that's not inside quotes
    let q_pos = find_unquoted(expr, '?')?;
    let condition = expr[..q_pos].trim();
    let rest = expr[q_pos + 1..].trim();

    // Find `:` that's not inside quotes
    let colon_pos = find_unquoted(rest, ':')?;
    let truthy = rest[..colon_pos].trim();
    let falsy = rest[colon_pos + 1..].trim();

    Some(Ternary {
        condition,
        truthy,
        falsy,
    })
}

/// Find the position of a character not inside double quotes.
fn find_unquoted(s: &str, target: char) -> Option<usize> {
    let mut in_quotes = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == target && !in_quotes {
            return Some(i);
        }
    }
    None
}

/// Remove surrounding double quotes from a string.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Split expression on unquoted `|` characters.
fn split_pipes(expr: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;

    for (i, c) in expr.char_indices() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == '|' && !in_quotes {
            parts.push(&expr[start..i]);
            start = i + 1;
        }
    }
    parts.push(&expr[start..]);
    parts
}

fn hostname() -> std::io::Result<String> {
    let output = std::process::Command::new("hostname").output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Backwards compatibility tests ===

    #[test]
    fn test_expand_date() {
        let vars = HashMap::new();
        let result = expand_template("backup-{{date}}.sql", &vars);
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert!(result.contains(&today));
    }

    #[test]
    fn test_expand_custom_vars() {
        let mut vars = HashMap::new();
        vars.insert("env".to_string(), "prod".to_string());
        vars.insert("db".to_string(), "mydb".to_string());

        let result = expand_template("deploy to {{env}} db={{db}}", &vars);
        assert_eq!(result, "deploy to prod db=mydb");
    }

    #[test]
    fn test_no_vars_passthrough() {
        let vars = HashMap::new();
        let result = expand_template("plain text", &vars);
        assert_eq!(result, "plain text");
    }

    #[test]
    fn test_unknown_var_passthrough() {
        let vars = HashMap::new();
        let result = expand_template("hello {{unknown}} world", &vars);
        assert_eq!(result, "hello {{unknown}} world");
    }

    // === Docker passthrough tests ===

    #[test]
    fn test_docker_format_passthrough() {
        let vars = HashMap::new();
        let result = expand_template("docker inspect --format '{{.State.Health}}'", &vars);
        assert_eq!(result, "docker inspect --format '{{.State.Health}}'");
    }

    #[test]
    fn test_go_template_range_passthrough() {
        let vars = HashMap::new();
        let result = expand_template("{{range .Items}}{{.Name}}{{end}}", &vars);
        assert_eq!(result, "{{range .Items}}{{.Name}}{{end}}");
    }

    // === Filter tests ===

    #[test]
    fn test_filter_upper() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "hello".to_string());
        let result = expand_template("{{name | upper}}", &vars);
        assert_eq!(result, "HELLO");
    }

    #[test]
    fn test_filter_lower() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "HELLO".to_string());
        let result = expand_template("{{name | lower}}", &vars);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_filter_trim() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "  hello  ".to_string());
        let result = expand_template("{{name | trim}}", &vars);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_filter_default_empty() {
        let vars = HashMap::new();
        let result = expand_template("{{missing | default \"fallback\"}}", &vars);
        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_filter_default_present() {
        let mut vars = HashMap::new();
        vars.insert("val".to_string(), "real".to_string());
        let result = expand_template("{{val | default \"fallback\"}}", &vars);
        assert_eq!(result, "real");
    }

    #[test]
    fn test_filter_replace() {
        let mut vars = HashMap::new();
        vars.insert("path".to_string(), "/old/path/old".to_string());
        let result = expand_template("{{path | replace \"old\" \"new\"}}", &vars);
        assert_eq!(result, "/new/path/new");
    }

    #[test]
    fn test_filter_truncate() {
        let mut vars = HashMap::new();
        vars.insert("msg".to_string(), "hello world".to_string());
        let result = expand_template("{{msg | truncate 5}}", &vars);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_filter_truncate_shorter() {
        let mut vars = HashMap::new();
        vars.insert("msg".to_string(), "hi".to_string());
        let result = expand_template("{{msg | truncate 5}}", &vars);
        assert_eq!(result, "hi");
    }

    #[test]
    fn test_filter_split_first() {
        let mut vars = HashMap::new();
        vars.insert("list".to_string(), "a,b,c".to_string());
        let result = expand_template("{{list | split \",\" | first}}", &vars);
        assert_eq!(result, "a");
    }

    #[test]
    fn test_filter_split_last() {
        let mut vars = HashMap::new();
        vars.insert("list".to_string(), "a,b,c".to_string());
        let result = expand_template("{{list | split \",\" | last}}", &vars);
        assert_eq!(result, "c");
    }

    #[test]
    fn test_filter_split_nth() {
        let mut vars = HashMap::new();
        vars.insert("list".to_string(), "a,b,c".to_string());
        let result = expand_template("{{list | split \",\" | nth 1}}", &vars);
        assert_eq!(result, "b");
    }

    #[test]
    fn test_filter_split_count() {
        let mut vars = HashMap::new();
        vars.insert("list".to_string(), "a,b,c".to_string());
        let result = expand_template("{{list | split \",\" | count}}", &vars);
        assert_eq!(result, "3");
    }

    #[test]
    fn test_filter_chain() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "  Hello World  ".to_string());
        let result = expand_template("{{name | trim | upper}}", &vars);
        assert_eq!(result, "HELLO WORLD");
    }

    // === Ternary tests ===

    #[test]
    fn test_ternary_eq_true() {
        let mut vars = HashMap::new();
        vars.insert("env".to_string(), "prod".to_string());
        let result = expand_template("{{env | eq \"prod\" ? \"production\" : \"other\"}}", &vars);
        assert_eq!(result, "production");
    }

    #[test]
    fn test_ternary_eq_false() {
        let mut vars = HashMap::new();
        vars.insert("env".to_string(), "dev".to_string());
        let result = expand_template("{{env | eq \"prod\" ? \"production\" : \"other\"}}", &vars);
        assert_eq!(result, "other");
    }

    // === date_offset tests ===

    #[test]
    fn test_date_offset_minus_1d() {
        let vars = HashMap::new();
        let result = expand_template("{{date_offset -1d}}", &vars);
        let yesterday = (chrono::Utc::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(result, yesterday);
    }

    #[test]
    fn test_date_offset_plus_7d() {
        let vars = HashMap::new();
        let result = expand_template("{{date_offset 7d}}", &vars);
        let future = (chrono::Utc::now() + chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(result, future);
    }

    #[test]
    fn test_date_offset_weeks() {
        let vars = HashMap::new();
        let result = expand_template("{{date_offset -2w}}", &vars);
        let past = (chrono::Utc::now() - chrono::Duration::weeks(2))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(result, past);
    }

    // === Mixed context tests ===

    #[test]
    fn test_mixed_vars_and_filters() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "world".to_string());
        let result = expand_template("Hello {{name | upper}}, today is {{date}}", &vars);
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(result, format!("Hello WORLD, today is {today}"));
    }

    #[test]
    fn test_docker_mixed_with_real_vars() {
        let mut vars = HashMap::new();
        vars.insert("container".to_string(), "nginx".to_string());
        let result = expand_template(
            "docker inspect {{container}} --format '{{.State.Status}}'",
            &vars,
        );
        assert_eq!(
            result,
            "docker inspect nginx --format '{{.State.Status}}'"
        );
    }

    // === Internal function tests ===

    #[test]
    fn test_parse_filter_name_args() {
        let (name, args) = parse_filter_name_args("replace \"old\" \"new\"");
        assert_eq!(name, "replace");
        assert_eq!(args, vec!["old", "new"]);
    }

    #[test]
    fn test_parse_filter_name_only() {
        let (name, args) = parse_filter_name_args("upper");
        assert_eq!(name, "upper");
        assert!(args.is_empty());
    }

    #[test]
    fn test_split_pipes() {
        let parts = split_pipes("var | upper | default \"x\"");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].trim(), "var");
        assert_eq!(parts[1].trim(), "upper");
        assert_eq!(parts[2].trim(), "default \"x\"");
    }

    #[test]
    fn test_split_pipes_no_split_in_quotes() {
        let parts = split_pipes("var | replace \"a|b\" \"c\"");
        assert_eq!(parts.len(), 2);
    }
}
