# MacroNest 1.2.5

## New features

- Added an intuitive **Set Trigger Key** hint button directly on unconfigured macro presets (outside of Window Focus mode), allowing one-click hotkey capture right from the preset header.
- Added **Instant Data Folder Switching** in Settings, dynamically applying and reloading all presets, profiles, and state without requiring an app restart.
- Added an **Instant Reset to Default Data Folder** button with live reloading and visual confirmation feedback (`Reset!` / `Đã đặt lại!`).
- Added full **Macro Preset & Group Sharing** code export and import (`Exp` / `Imp`) with compact encoding and clipboard toast notifications.
- Added MacroNest desktop wallpapers to the themes collection.
- Added Copy and Paste controls to preset card headers across all categories.

## Improvements

- Accelerated Quick Video Recording startup using fast GDI capture (starts in under 50ms).
- Synchronized the yellow recording border with the live timer badge and added a Preparing state transition indicator.
- Synchronized WASAPI audio recording with frame 0, eliminating stream analysis delays and initial video freeze.
- Enhanced Video Library responsiveness with smooth seeking, 60 FPS previews, full library view, playhead navigation, and audio track caching.
- Window Focus mode macros now trigger immediately upon preset enablement if the target window is already active.
- Added expression highlighting for `SetVariable` fields and live expression evaluation inside HUD text.
- Reduced idle RAM working set and CPU footprint during background operation.
- Expanded Vietnamese and English localization across all macro and settings interfaces.

## Bug fixes

- Fixed macro import issues: preserved variable names, window title selectors, and enabled/disabled preset references across groups.
- Fixed zero-delay infinite macro loops causing UI freezes.
- Fixed macro trigger keys occasionally failing immediately after switching windows (Alt-Tab).
- Fixed TypeText Paste mode not saving or executing correctly.
- Fixed black HUD text visibility and text-edit bottom border clipping.
- Fixed disappearing Pin selection borders and drawing toolbar flicker.
- Fixed permanent geometry overlay duration and eliminated redundant repaints.
