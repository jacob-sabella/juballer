# Style guide

Authoritative reference for code style across the workspace. The
tooling described here is what `scripts/check.sh` (and CI) enforce —
everything else is convention that reviewers can flag but won't break
the build.

## Toolchain

The toolchain is pinned by `rust-toolchain.toml`. Use it; don't pass
`+nightly` unless a specific task demands it. All configuration in
this repo targets **stable**.

## Formatting (`rustfmt.toml`)

- `edition = 2021`
- `max_width = 100` — the default. Anything wider is a code smell
  before it's a formatting question.
- `tab_spaces = 4`, `hard_tabs = false`, `newline_style = "Unix"`.
- Imports and modules auto-reorder on `cargo fmt`.

`cargo fmt --all` is the only acceptable formatter. Do not hand-align
`struct` fields or `match` arms.

## Lints (`[workspace.lints]` in `Cargo.toml`)

The workspace enables:

- `rust_2018_idioms` + `nonstandard_style` (warn)
- `missing_debug_implementations`, `unreachable_pub`,
  `unsafe_op_in_unsafe_fn` (warn)
- `clippy::all` + `clippy::pedantic` (warn)

CI runs `cargo clippy ... -- -D warnings`, so anything that warns in
the listed groups fails the build.

A short allow-list in `[workspace.lints.clippy]` disables pedantic
rules that fire mostly on f32 / render code (casts, similar names,
etc.). If you find yourself sprinkling `#[allow(clippy::…)]` inside
modules, prefer widening the workspace allow-list instead — keep the
lint policy in one file.

## Comments and docs

### Module-level

Every `.rs` file starts with a `//!` module comment explaining **what
the module is responsible for** in 1–3 sentences. Skip restating
things that are obvious from the types.

### Items

Public items get a `///` doc comment. The first line is a one-sentence
summary ending in a period. A blank line, then whatever detail is
genuinely useful — invariants, failure modes, why a particular shape
was chosen.

```rust
/// Decode a memon long-note tail from the `p` field.
///
/// Returns the `(row, col)` of the tail cell. `p` values outside the
/// 0..=5 range fall back to the head cell with no warning.
pub fn resolve_tail(row: u8, col: u8, p: Option<u8>) -> (u8, u8) { … }
```

Private items can be documented when the behaviour is non-obvious;
otherwise a descriptive name is enough.

### Inline comments

Use sparingly, and only to explain **why**. If a comment is restating
what the next line does, delete it. Good inline comments call out:

- Hidden invariants ("callers rely on this being sorted").
- Workarounds for external bugs (link to the upstream issue).
- Non-obvious performance trade-offs.
- Subtle correctness concerns.

Never leave commentary about the editing session (e.g. "fixed in this
pass", "added per the review") or AI-generated narration. Comments
describe the code that exists, not the path taken to get there.

## File layout

- Imports grouped in blocks: `std`, external crates, workspace
  crates, `crate::`. Blank line between each group. `cargo fmt`
  preserves intra-group order.
- `mod` declarations and their `pub use` re-exports sit at the top.
- Tests go in `#[cfg(test)] mod tests { … }` at the bottom of the
  file they test.

## Naming

Rust standard conventions apply: `snake_case` for functions, fields,
and modules; `UpperCamelCase` for types and enum variants;
`SCREAMING_SNAKE_CASE` for `const` / `static`. No Hungarian prefixes,
no type suffixes on variables.

## Errors

Prefer `thiserror` for library error enums, `anyhow` for application
code. A public function that can fail returns `Result<T, E>` with a
concrete `E`; don't leak `Box<dyn Error>` across crate boundaries.

## Unsafe

`unsafe` blocks carry a `// SAFETY:` comment immediately above them
that justifies every condition the surrounding `unsafe fn` or
`unsafe { … }` requires. `unsafe_op_in_unsafe_fn` is enabled so that
`unsafe fn` bodies don't get a free pass.
