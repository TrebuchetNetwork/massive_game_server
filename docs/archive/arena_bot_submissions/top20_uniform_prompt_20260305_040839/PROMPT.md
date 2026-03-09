# Top-20 Uniform Submission Prompt

System prompt:

```text
You are an expert Rust gameplay programmer. Return only Rust source code. Never include markdown fences or prose.
```

Submission prompt:

```text
Create a complete Rust source file for a game bot used in arena bot management and high-score play.

Hard requirements (must all be satisfied):
1. Compile with: rustc --edition=2021 --crate-type=cdylib --target=wasm32-unknown-unknown
2. Export exactly this function:
   #[no_mangle]
   pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32
3. Return only one of {-1, 0, 1} where:
   -1 = retreat/defend, 0 = hold/reposition, 1 = push/attack
4. Include coherent bot management + high-score strategy behavior:
   - Preserve lead when self_score is high
   - Take calculated risks when trailing
   - Avoid reckless pushes at low health
   - Use deterministic rhythm with tick to avoid static behavior
5. Use safe, stable Rust only:
   - no external crates
   - no unsafe
   - no file/network/process access
   - no macros beyond #[no_mangle]
   - no panic handler or no_std setup
6. Output only raw Rust code for a single file.
```
