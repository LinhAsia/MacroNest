# Dead Code Audit

Date: 2026-06-17

## Scope

This audit is intentionally read-only for runtime behavior.
It identifies likely dead code, UI-hidden dead paths, and duplicated logic candidates
without removing anything from the app yet.

Current snapshot from `python scripts/audit_dead_code.py`:

- Rust files scanned: 38
- Rust source lines scanned: 83,812
- Compiler signals found:
  - 66 `never used`
  - 9 `never read`
  - 7 `unreachable`

## First findings

### High-confidence dead or abandoned paths

- `src/ui/crosshair_panel.rs:7`
  - `render_crosshair_panel` calls `render_crosshair_presets_panel(ui);` and immediately `return;`.
  - Everything below that return is unreachable and should be treated as dead until proven otherwise.

- `src/ui/macro_panel.rs.bak`
  - Large backup source file checked into the repo.
  - Not referenced anywhere by Rust modules.
  - High-confidence dead weight and a source of audit noise.

- Compiler-reported never-used helpers already present in the current codebase:
  - `src/ui/window_panel.rs:405`
  - `src/ui/window_panel.rs:1070`
  - `src/overlay.rs:16808`
  - `src/ui/settings_panel.rs:1271`
  - `src/audio.rs:272`
  - `src/profile_code.rs:10`

### Hidden UI dead-code candidates

These are riskier than compiler warnings because the code may still be reachable in odd UI flows,
but they are strong suspects:

- `src/ui/window_panel.rs`
  - `render_zoom_panel`
  - `render_modes_panel`
  - `add_window_focus_preset`
  - `add_zoom_preset`
  - `zoom_preview_for_preset`
  - `apply_locked_aspect_ratio`

- `src/ui/vision_panel.rs`
  - Multiple associated items marked `never used` by the compiler.
  - This strongly suggests older capture/preview flows may have been replaced without removing old code paths.

- `src/ui/mod.rs`
  - The compiler reports "multiple associated items are never used".
  - This usually means former panel helpers or UI utilities still exist after route changes.

### Possible duplicated logic clusters

- `src/overlay.rs` vs `src/overlay/drawing.rs`
  - Similar drawing primitives exist in more than one place.
  - Candidate for "same feature, second implementation" drift.

- Window preset application in `src/overlay.rs`
  - `apply_window_preset_by_id`
  - `apply_window_preset_for_macro`
  - `apply_window_preset`
  - `apply_window_preset_impl`
  - `apply_window_preset_animated`
  - Needs a route audit to confirm which paths are live and which are legacy.

## Safe cleanup order

1. Remove repo residue that does not participate in compilation.
   - Example: `src/ui/macro_panel.rs.bak`

2. Remove unreachable blocks with explicit proof.
   - Example: code after the early `return` in `src/ui/crosshair_panel.rs`

3. Remove compiler-proven unused helpers in small batches.
   - Build after each batch.

4. Audit UI routes before deleting larger "never used" panel helpers.
   - Confirm there is no route from panel selection, modal buttons, quick actions, or overlay commands.

5. Merge duplicated helpers only after the live path is identified.

## Recommended process

- Run `python scripts/audit_dead_code.py`
- Review `output/dead_code_audit.json`
- Pick one cluster at a time
- Delete the entire dead path, not just hide the UI
- Run `cargo build --release` after each cleanup batch
