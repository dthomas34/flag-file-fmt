use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::process;

#[derive(Debug, Clone)]
struct Rule {
    key: String,
    value: String,
}

#[derive(Debug, Clone)]
struct Flag {
    name: String,
    enabled: bool,
    rollout: Option<u8>,
    rules: Vec<Rule>,
}

#[derive(Debug)]
struct ParseError {
    line: usize,
    message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

fn err(line: usize, message: impl Into<String>) -> ParseError {
    ParseError { line, message: message.into() }
}

fn is_valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// Splits on `delim` at bracket depth zero, so "rules=[a=b, c=d]" survives a
// top-level split on ',' without breaking apart the rule list inside it.
fn split_top_level(s: &str, delim: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in s.chars() {
        match c {
            '[' => {
                depth += 1;
                current.push(c);
            }
            ']' => {
                depth -= 1;
                current.push(c);
            }
            c if c == delim && depth == 0 => {
                parts.push(current.trim().to_string());
                current = String::new();
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() || !parts.is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn parse(source: &str) -> Result<Vec<Flag>, ParseError> {
    let mut flags = Vec::new();
    let mut seen = HashSet::new();

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let rest = line
            .strip_prefix("flag ")
            .ok_or_else(|| err(line_no, "expected line to start with 'flag '"))?;
        let colon = rest
            .find(':')
            .ok_or_else(|| err(line_no, "missing ':' after flag name"))?;
        let name = rest[..colon].trim().to_string();
        if !is_valid_name(&name) {
            return Err(err(
                line_no,
                format!(
                    "invalid flag name '{}': use lowercase letters, digits, and hyphens, starting with a letter",
                    name
                ),
            ));
        }
        if !seen.insert(name.clone()) {
            return Err(err(line_no, format!("duplicate flag name '{}'", name)));
        }

        let body = rest[colon + 1..].trim();
        let segments = split_top_level(body, ',');
        if segments.is_empty() || segments[0].is_empty() {
            return Err(err(line_no, "expected 'on' or 'off' after flag name"));
        }

        let enabled = match segments[0].as_str() {
            "on" => true,
            "off" => false,
            other => {
                return Err(err(line_no, format!("expected 'on' or 'off', found '{}'", other)))
            }
        };

        let mut rollout = None;
        let mut rules = Vec::new();

        for segment in &segments[1..] {
            let eq = segment
                .find('=')
                .ok_or_else(|| err(line_no, format!("expected 'key=value' in '{}'", segment)))?;
            let key = segment[..eq].trim();
            let value = segment[eq + 1..].trim();

            match key {
                "rollout" => {
                    let n: u8 = value.parse().map_err(|_| {
                        err(line_no, format!("rollout must be an integer 0-100, found '{}'", value))
                    })?;
                    if n > 100 {
                        return Err(err(line_no, format!("rollout must be 0-100, found {}", n)));
                    }
                    rollout = Some(n);
                }
                "rules" => {
                    let inner = value
                        .strip_prefix('[')
                        .and_then(|v| v.strip_suffix(']'))
                        .ok_or_else(|| err(line_no, "rules must be wrapped in [ ]"))?;
                    for rule_seg in split_top_level(inner, ',') {
                        if rule_seg.is_empty() {
                            return Err(err(line_no, "empty entry in rules list"));
                        }
                        let req = rule_seg.find('=').ok_or_else(|| {
                            err(line_no, format!("expected 'key=value' in rule '{}'", rule_seg))
                        })?;
                        let rkey = rule_seg[..req].trim().to_string();
                        let rvalue = rule_seg[req + 1..].trim().to_string();
                        if rkey.is_empty() || rvalue.is_empty() {
                            return Err(err(
                                line_no,
                                format!("rule '{}' has an empty key or value", rule_seg),
                            ));
                        }
                        rules.push(Rule { key: rkey, value: rvalue });
                    }
                }
                other => return Err(err(line_no, format!("unknown attribute '{}'", other))),
            }
        }

        flags.push(Flag { name, enabled, rollout, rules });
    }

    Ok(flags)
}

fn format_flags(flags: &[Flag]) -> String {
    let mut out = String::new();
    for flag in flags {
        out.push_str("flag ");
        out.push_str(&flag.name);
        out.push_str(": ");
        out.push_str(if flag.enabled { "on" } else { "off" });
        if let Some(r) = flag.rollout {
            out.push_str(&format!(", rollout={}", r));
        }
        if !flag.rules.is_empty() {
            out.push_str(", rules=[");
            let parts: Vec<String> =
                flag.rules.iter().map(|r| format!("{}={}", r.key, r.value)).collect();
            out.push_str(&parts.join(", "));
            out.push(']');
        }
        out.push('\n');
    }
    out
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        let prog = args.get(0).map(String::as_str).unwrap_or("flagfmt");
        eprintln!("usage: {} <check|fmt> <path>", prog);
        process::exit(2);
    }
    let command = &args[1];
    let path = &args[2];

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path, e);
            process::exit(2);
        }
    };

    let flags = match parse(&source) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            process::exit(1);
        }
    };

    match command.as_str() {
        "check" => println!("{}: {} flag(s) OK", path, flags.len()),
        "fmt" => print!("{}", format_flags(&flags)),
        other => {
            eprintln!("unknown command '{}', expected 'check' or 'fmt'", other);
            process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_flag() {
        let flags = parse("flag dark-mode: on\n").unwrap();
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].name, "dark-mode");
        assert!(flags[0].enabled);
        assert_eq!(flags[0].rollout, None);
        assert!(flags[0].rules.is_empty());
    }

    #[test]
    fn parses_full_flag() {
        let src = "flag new-checkout: on, rollout=25, rules=[env=staging, plan=enterprise]\n";
        let flags = parse(src).unwrap();
        assert_eq!(flags[0].rollout, Some(25));
        assert_eq!(flags[0].rules.len(), 2);
        assert_eq!(flags[0].rules[0].key, "env");
        assert_eq!(flags[0].rules[0].value, "staging");
        assert_eq!(flags[0].rules[1].value, "enterprise");
    }

    #[test]
    fn rejects_duplicate_names() {
        let src = "flag a: on\nflag a: off\n";
        assert!(parse(src).is_err());
    }

    #[test]
    fn rejects_bad_rollout() {
        assert!(parse("flag a: on, rollout=101\n").is_err());
        assert!(parse("flag a: on, rollout=nope\n").is_err());
    }

    #[test]
    fn rejects_bad_name() {
        assert!(parse("flag Bad_Name: on\n").is_err());
        assert!(parse("flag -leading-hyphen: on\n").is_err());
    }

    #[test]
    fn rejects_unknown_attribute() {
        assert!(parse("flag a: on, sunset=2027\n").is_err());
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let src = "# a comment\n\nflag a: on\n";
        let flags = parse(src).unwrap();
        assert_eq!(flags.len(), 1);
    }

    #[test]
    fn format_round_trips_canonical_input() {
        let src = "flag a: on, rollout=10, rules=[env=prod]\n";
        let flags = parse(src).unwrap();
        assert_eq!(format_flags(&flags), src);
    }

    #[test]
    fn format_normalizes_loose_spacing() {
        let src = "flag a:on,rollout=10,rules=[env=prod,plan=pro]\n";
        let flags = parse(src).unwrap();
        assert_eq!(format_flags(&flags), "flag a: on, rollout=10, rules=[env=prod, plan=pro]\n");
    }
}
