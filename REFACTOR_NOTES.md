# Refactor Notes

## Current Architecture Map

- UI panels: `src/ui/` contains the `CrosshairApp` shell plus panel-specific rendering and interaction code for macro, mouse, OCR, vision, sound, settings, window, HUD, and command flows.
- Overlay runtime and rendering: `src/overlay/` owns the always-on-top runtime, native capture bridge, rendering paths, hit testing, and overlay-side command handling.
- Macro engine and editor: macro configuration lives in `src/model/`, editor and runner UI live in `src/ui/macro_panel.rs`, and code generation helpers live in `src/macro_code.rs`.
- OCR and vision: OCR presets and matching logic span `src/ocr.rs`, `src/model/`, `src/ui/ocr_panel.rs`, `src/ui/vision_panel.rs`, and `src/overlay/vision.rs`.
- Audio and audio sense: playback/configuration logic spans `src/audio.rs`, `src/audiosense.rs`, `src/ui/sound_panel.rs`, and `src/overlay/audio_sense.rs`.
- Storage and model: persisted state is defined in `src/model/`, with load/save and migration logic in `src/storage.rs`.
- Platform, native, window, and hotkey: Windows-specific process, restart, elevation, hotkey, and window enumeration code lives in `src/platform.rs`, `src/hotkey.rs`, and `src/window_list.rs`.

## Risk Areas

- Hotkey and global input hooks are behavior-critical and easy to regress with lifetime or threading changes.
- Overlay topmost, transparency, click-through, drag, and resize paths are platform-specific and should only change behind compile-safe refactors.
- Native capture and vision bridging are sensitive to timing, HWND state, and GPU/native APIs.
- OCR and vision pipelines can easily regress performance if work leaks back into the UI thread.
- Macro execution must preserve timing, serialization format, and side-effect order.
- Storage migration and serde defaults must remain backward compatible with existing user data.
- Audio playback and audio capture should avoid blocking UI and should keep current caching/runtime behavior stable.

## Build Baseline

- Baseline commands for this refactor slice:
  - `cargo fmt`
  - `cargo check`
  - `cargo clippy --all-targets`
- Current repo already has a large existing warning surface; this refactor slice should not expand scope into unrelated cleanup unless a warning blocks compilation.
- Status after this phase:
  - `cargo fmt`: passed
  - `cargo check`: passed with the existing warning-heavy baseline
  - `cargo clippy --all-targets`: passed with the existing warning-heavy baseline

## This Phase

- Converted `src/model.rs` into `src/model/mod.rs` without changing the public `crate::model::*` API. This creates the directory entrypoint needed for future domain splits.
- Converted `src/overlay.rs` into `src/overlay/mod.rs` without changing overlay behavior. This creates the directory entrypoint needed for future runtime/render/input extraction.
- Converted the largest UI panel files into directory entrypoints:
  - `src/ui/macro_panel.rs` -> `src/ui/macro_panel/mod.rs`
  - `src/ui/mouse_panel.rs` -> `src/ui/mouse_panel/mod.rs`
  - `src/ui/vision_panel.rs` -> `src/ui/vision_panel/mod.rs`
  - `src/ui/window_panel.rs` -> `src/ui/window_panel/mod.rs`
- Split `src/model/mod.rs` by domain without changing the `crate::model::*` API:
  - [src/model/overlay_model.rs](D:/app/MacroNest/src/model/overlay_model.rs)
  - [src/model/settings_model.rs](D:/app/MacroNest/src/model/settings_model.rs)
  - [src/model/audio_model.rs](D:/app/MacroNest/src/model/audio_model.rs)
  - [src/model/window_model.rs](D:/app/MacroNest/src/model/window_model.rs)
  - [src/model/geometry_model.rs](D:/app/MacroNest/src/model/geometry_model.rs)
  - [src/model/vision_model.rs](D:/app/MacroNest/src/model/vision_model.rs)
  - [src/model/macro_model.rs](D:/app/MacroNest/src/model/macro_model.rs)
- Finished the remaining `model` holdouts:
  - mouse path and mouse sensitivity presets now live in [src/model/window_model.rs](D:/app/MacroNest/src/model/window_model.rs)
  - HUD, timer, command, AI settings, and `AppState` now live in [src/model/settings_model.rs](D:/app/MacroNest/src/model/settings_model.rs)
  - `src/model/mod.rs` now only keeps shared defaults and module wiring/re-exports
- Started the `ui/mod.rs` reduction with a low-risk extraction:
  - moved font loading, CJK fallback detection, and theme visual configuration into [src/ui/theme.rs](D:/app/MacroNest/src/ui/theme.rs)
  - kept the public `crate::ui::configure_fonts` and `crate::ui::configure_theme` entrypoints stable via re-export
- Continued the `ui/mod.rs` reduction with shared helper extraction:
  - moved shared preset/card/button hover helpers into [src/ui/widgets.rs](D:/app/MacroNest/src/ui/widgets.rs)
  - moved titlebar/tab navigation labels and button styling into [src/ui/navigation.rs](D:/app/MacroNest/src/ui/navigation.rs)
  - moved startup overlay sync, state-to-overlay sync, and persist helpers into [src/ui/state_sync.rs](D:/app/MacroNest/src/ui/state_sync.rs)
  - kept the existing `CrosshairApp` call sites unchanged by reusing `impl CrosshairApp` methods from the new modules
- Added small behavior-locking tests for the refactor slice:
  - legacy alias deserialization for `AppState`
  - mouse sensitivity default restore settings
- Kept behavior-preserving changes first; no serialization rename or user-facing flow change was introduced in this phase.

## Next Safe Steps

- Continue extracting shared UI shell helpers from `src/ui/mod.rs` into smaller modules without changing layout or styling.
- Good next cuts are remaining pure helper clusters such as copy/share feedback utilities, window-title formatting, capture-info placement, and titlebar quick-action rendering.
- Keep starting with pure/testable helpers before touching overlay runtime behavior.
