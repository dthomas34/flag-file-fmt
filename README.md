# flagfmt

A validating parser and pretty printer for a small text format that describes
feature flags. The problem it's for: flag files get hand-edited by whoever is
shipping that week, and over time the spacing drifts, rollout percentages
sneak past 100, someone typos a flag name, or two people add the same flag on
different branches. None of that shows up until something breaks in
production. `flagfmt` catches the structural mistakes and reformats the file
into one consistent shape, the same idea as `gofmt` but for a flags file
instead of source code.

## The format

One flag per line. Comments start with `#`. Blank lines are ignored.

```
# flags for the checkout service
flag new-checkout: on, rollout=25, rules=[env=staging, plan=enterprise]
flag legacy-search: off
flag dark-mode: on, rollout=100
```

Grammar, informally:

```
flag <name>: <on|off>[, rollout=<0-100>][, rules=[<key>=<value>, ...]]
```

Rules:

- `<name>` is lowercase letters, digits, and hyphens, and must start with a
  letter (`new-checkout`, `beta2`, not `Beta_2` or `-beta`).
- a flag name can only appear once in the file.
- the state must be exactly `on` or `off`.
- `rollout` is optional and must be an integer from 0 to 100.
- `rules` is optional and holds a list of `key=value` targeting conditions;
  the meaning of each key (`env`, `plan`, whatever else) is up to whoever
  reads the file downstream, `flagfmt` only checks the shape.

## Usage

```
cargo run -- check examples/flags.example.txt
cargo run -- fmt examples/flags.example.txt
```

`check` parses and validates the file, printing an error with a line number
and exiting non-zero on the first problem it finds. `fmt` parses the file and
prints the canonical formatting to stdout, normalizing spacing around commas,
colons, and brackets, so it can be piped to a new file or diffed against the
original:

```
cargo run -- fmt flags.txt > flags.fmt.txt
diff flags.txt flags.fmt.txt
```

Pass `--write` to reformat the file in place instead:

```
cargo run -- fmt --write flags.txt
```

Given `flag a:on,rollout=10,rules=[env=prod,plan=pro]`, `fmt` produces
`flag a: on, rollout=10, rules=[env=prod, plan=pro]`.

## Status

This is a first pass. There's no diff-style check mode yet for CI (`check`
only validates, it doesn't report formatting drift), and the format has no
concept of environment-scoped overrides. See the roadmap in the commit
history for what's next.

## License

MIT, see LICENSE.
