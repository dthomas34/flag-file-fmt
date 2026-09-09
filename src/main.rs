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

// One entry per source line, in file order, so formatting can reproduce the
// author's comments and paragraph breaks instead of collapsing them away.
#[derive(Debug, Clone)]
enum Entry {
    Blank,
    Comment(String),
    Flag(Flag),
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

fn parse(source: &str) -> Result<Vec<Entry>, ParseError> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            entries.push(Entry::Blank);
            continue;
        }
        if line.starts_with('#') {
            entries.push(Entry::Comment(line.to_string()));
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

        entries.push(Entry::Flag(Flag { name, enabled, rollout, rules }));
    }

    Ok(entries)
}

fn format_flag(flag: &Flag, out: &mut String) {
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

fn format_entries(entries: &[Entry]) -> String {
    let mut out = String::new();
    for entry in entries {
        match entry {
            Entry::Blank => out.push('\n'),
            Entry::Comment(c) => {
                out.push_str(c);
                out.push('\n');
            }
            Entry::Flag(flag) => format_flag(flag, &mut out),
        }
    }
    out
}

fn usage(prog: &str) -> String {
    format!("usage: {} <check|fmt> [--write] <path>", prog)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.get(0).map(String::as_str).unwrap_or("flagfmt");
    if args.len() < 3 {
        eprintln!("{}", usage(prog));
        process::exit(2);
    }
    let command = args[1].clone();

    let mut write_in_place = false;
    let mut path: Option<&str> = None;
    for arg in &args[2..] {
        if arg == "--write" {
            write_in_place = true;
        } else if path.is_none() {
            path = Some(arg);
        } else {
            eprintln!("{}", usage(prog));
            process::exit(2);
        }
    }
    let path = match path {
        Some(p) => p,
        None => {
            eprintln!("{}", usage(prog));
            process::exit(2);
        }
    };
    if write_in_place && command != "fmt" {
        eprintln!("--write is only valid with 'fmt'");
        process::exit(2);
    }

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path, e);
            process::exit(2);
        }
    };

    let entries = match parse(&source) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            process::exit(1);
        }
    };

    match command.as_str() {
        "check" => {
            let count = entries.iter().filter(|e| matches!(e, Entry::Flag(_))).count();
            println!("{}: {} flag(s) OK", path, count);
        }
        "fmt" => {
            let formatted = format_entries(&entries);
            if write_in_place {
                if let Err(e) = fs::write(path, &formatted) {
                    eprintln!("failed to write {}: {}", path, e);
                    process::exit(2);
                }
            } else {
                print!("{}", formatted);
            }
        }
        other => {
            eprintln!("unknown command '{}', expected 'check' or 'fmt'", other);
            process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags_only(entries: &[Entry]) -> Vec<&Flag> {
        entries
            .iter()
            .filter_map(|e| match e {
                Entry::Flag(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn parses_minimal_flag() {
        let entries = parse("flag dark-mode: on\n").unwrap();
        let flags = flags_only(&entries);
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].name, "dark-mode");
        assert!(flags[0].enabled);
        assert_eq!(flags[0].rollout, None);
        assert!(flags[0].rules.is_empty());
    }

    #[test]
    fn parses_full_flag() {
        let src = "flag new-checkout: on, rollout=25, rules=[env=staging, plan=enterprise]\n";
        let entries = parse(src).unwrap();
        let flags = flags_only(&entries);
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
    fn keeps_comments_and_blank_lines_as_entries() {
        let src = "# a comment\n\nflag a: on\n";
        let entries = parse(src).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], Entry::Comment(c) if c == "# a comment"));
        assert!(matches!(entries[1], Entry::Blank));
        assert!(matches!(&entries[2], Entry::Flag(f) if f.name == "a"));
    }

    #[test]
    fn format_round_trips_canonical_input() {
        let src = "flag a: on, rollout=10, rules=[env=prod]\n";
        let entries = parse(src).unwrap();
        assert_eq!(format_entries(&entries), src);
    }

    #[test]
    fn format_normalizes_loose_spacing() {
        let src = "flag a:on,rollout=10,rules=[env=prod,plan=pro]\n";
        let entries = parse(src).unwrap();
        assert_eq!(
            format_entries(&entries),
            "flag a: on, rollout=10, rules=[env=prod, plan=pro]\n"
        );
    }

    #[test]
    fn format_preserves_comments_and_blank_lines() {
        let src = "# checkout flags\n\nflag a:on\n# trailing note\n";
        let entries = parse(src).unwrap();
        assert_eq!(
            format_entries(&entries),
            "# checkout flags\n\nflag a: on\n# trailing note\n"
        );
    }
}
