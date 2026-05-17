# Plan 06: Remove MAX_PTS limit

## Goal
Remove the `MAX_PTS` const generic parameter from `parse_hpgl_async` and all related code. The streaming parser emits points one-by-one via `handler.pen_down_point()` — there is no buffer accumulating them, so the limit is unnecessary.

## Steps

### 1. Remove `MAX_PTS` from `feed_byte` and `handle_pd_byte`
- Delete the `const MAX_PTS: usize` generic parameter from both methods.
- Remove all `if self.pd_point_count >= MAX_PTS { return Err(InnerError::TooManyPoints); }` checks (2 occurrences in `handle_pd_byte`).

### 2. Remove `pd_point_count` field from `StateMachine`
- Delete the field declaration and its doc comment.
- Remove initialization (`pd_point_count: 0`) from `new()`.
- Remove all `self.pd_point_count` assignments/resets throughout the struct methods:
  - `handle_command_byte`: `self.pd_point_count = 1` / `= 0`
  - `handle_pd_byte`: `+= 1`, `= 0`
  - `flush_delim`: `= 0`

### 3. Remove `InnerError::TooManyPoints` variant
- Delete the variant from `InnerError` enum.
- Remove corresponding arm in `From<InnerError> for ParseError<E>`.

### 4. Remove `ParseError::TooManyPoints` variant
- Delete from `ParseError<E>` enum.

### 5. Remove `MAX_PTS` from `parse_hpgl_async` signature
- Delete `const MAX_PTS: usize` from the generic parameters.
- Update the turbofish call to `feed_byte` — remove the `MAX_PTS` const generic.

### 6. Remove `DEFAULT_MAX_PTS` constant
- Delete the public constant and its doc comment.

### 7. Update tests
- Remove all `DEFAULT_MAX_PTS` imports and usages from `async_parser_tests.rs`.
- Update turbofish calls: `parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(...)` → `parse_hpgl_async::<DEFAULT_DELIM, _, _>(...)`.
- Delete or rewrite `async_too_many_points_returns_err` test (it tests removed functionality).
- Delete or rewrite `async_pd_large_streams_correctly` test if it only existed to verify MAX_PTS headroom (keep if it tests general large-stream correctness — just remove turbofish MAX_PTS).

### 8. Update hamego-wasm if it uses MAX_PTS
- Check `hamego-wasm/src/lib.rs` for `DEFAULT_MAX_PTS` usage and update accordingly.

### 9. Verify
```sh
cargo fmt
cargo clippy
cargo test -p hamego-core
cargo test  # workspace-wide
```

## Files to modify
- `hamego-core/src/async_parser.rs` — steps 1–6
- `hamego-core/tests/async_parser_tests.rs` — step 7
- `hamego-wasm/src/lib.rs` — step 8 (if applicable)

