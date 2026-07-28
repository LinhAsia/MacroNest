# MacroNest 1.2.4

## New features

- Added dynamic variable names and the `charat()` expression function.
- Added support for assigning and comparing whitespace or special characters.
- Added total App Data size information.
- Added panning for zoomed Pin and Source Crop previews.
- Added native opacity control for Pin presets.
- Added native Focus Mode with focused or selected window targeting and adjustable dimming.
- Added native Window Opacity Quick Action with focused or selected window targeting.
- Added a native Quick Video Recorder with full-screen, window, and region capture; configurable FPS; system audio; hold-trigger recording; and immediate recording feedback.
- Added a Video Library with embedded playback, audio, thumbnails, playhead seeking, timeline trimming, target-size compression, copy, delete, and file-location actions.
- Added Windows startup options for launching MacroNest automatically.
- Added `ReadMemory` and `WriteMemory` macro actions with numeric types, text values, direct addresses, module offsets, saved names, pointer chains, and value expressions.
- Added a complete Memory Scanner workspace with exact and unknown scans, changed/unchanged/increased/decreased filters, numeric and text value types, scan ranges, result marking, hotkeys, freezing, and manual addresses.
- Added Memory inspection tools including the memory-region viewer, editable structure view, class-style viewer, disassembler, module/DLL browser, instruction access/write tracing, persistent code list, and NOP/restore actions.
- Added automatic stable-pointer search and deep multi-pointer-map comparison, including restart validation, live resolved values, saved pointers, and reusable module-plus-offset expressions.
- Added saved Memory addresses and pointers grouped by process, with load, delete, pin, multi-select, and context-menu operations.
- Added x86 and x64 Memory debugger support with selectable or automatic architecture detection.
- Added a Network recorder with HTTP proxy capture, optional HTTPS decryption, reusable local CA management, WebSocket inspection, host/path hierarchy, filtering, request details, and pinned monitoring.
- Added optional Frida-based Network tracing as a Downloaded Tool instead of bundling it into every MacroNest installation.
- Added Copy and Paste controls to preset cards across Commands, Window Control, Pin, Mouse, Vision, AudioSense, OCR, Geometry, Media, HUD, and Timer.

## Improvements

- Improved Gemini response speed with web search support.
- Reduced the main executable from approximately 107 MB to approximately 42 MB by moving Frida into the optional Downloaded Tools workflow.
- Greatly improved Memory scan performance with parallel filtering, working-set scanning, compact result storage, uncapped result counts, stable scan baselines, and refreshes limited to visible rows.
- Improved Memory process selection with grouped windows, individual PIDs, executable paths, lazy icons, virtualized rows, and automatic stale-process clearing.
- Improved Memory tables, popups, and pinned windows with fixed left-aligned columns, full-row hitboxes, Windows-style Ctrl/Shift selection, resizing constraints, selectable text, and consistent custom title bars.
- Improved pointer workflows with broader region coverage, faster candidate validation, searchable results, EXE filtering, live values, and direct Add-to-Address-list actions.
- Improved the debugger by limiting rendered captures, enforcing a single active session, rearming breakpoints safely, and handling WOW64 and hardware single-step events more reliably.
- Improved Network reliability with native proxy configuration, automatic proxy restoration on stop and exit, resilient CONNECT forwarding, safer TLS failure handling, and deferred startup checks.
- Improved video recording startup speed, region selection, audio capture, encoder capability caching, and recording status synchronization.
- Improved Video Library responsiveness with cached thumbnails, prebuffered playback and audio, reusable preview sessions, faster random seeking, and trim-range changes that do not reload the video.
- Added per-step Mouse Click Delay and made Memory hotkeys use the same global trigger path as macro hotkeys.
- Imported macro groups now stay inside the currently open folder.
- Preserved Crosshair preset positions when saving and reopening the app.
- Unified macro-step and Quick Action typography so controls no longer change height because of mismatched font sizes.
- Added expression highlighting for SetVariable fields and expression evaluation inside HUD text.
- Made HUD changes appear immediately instead of waiting for the normal overlay refresh timer.
- Expanded Vietnamese localization across Memory, Vision, Downloaded Tools, popup title bars, and context-menu actions.

## Bug fixes

- Fixed TypeText Paste mode not saving or working correctly.
- Fixed zero-delay infinite macro loops starving the UI and causing hangs or deadlock-like crashes.
- Fixed macro trigger keys occasionally failing immediately after switching to another window.
- Fixed per-step mouse click delay not being honored correctly by click actions.
- Fixed Quick Video Recorder startup, trigger release, hold-stop behavior, missing preparing state, audio synchronization, and delayed active-recording feedback.
- Fixed Video Library preview audio, slow playhead seeking, trim-handle conflicts, loading-state layout shifts, playback-end loading flashes, clipped bounds, card alignment, copy feedback, and video deletion.
- Fixed Memory row selection, Ctrl+A, Shift selection anchors, double-click editing, right-click targeting, full-width row hitboxes, focus routing, Delete-key handling, and multi-address operations.
- Fixed Memory value, description, address, pointer, text, and arithmetic-expression editing not committing reliably.
- Fixed frozen Memory values updating only while MacroNest was focused by moving freeze writes to the background.
- Fixed pointer scans missing valid regions, losing candidates after restart, or validating against stale values.
- Fixed debugger crashes and target-process termination caused by unsafe breakpoint cleanup, WOW64 exception handling, concurrent debugger sessions, and capture auto-stop.
- Fixed module enumeration failing on transient `ERROR_MORE_DATA` responses.
- Fixed Network capture breaking internet access, leaving the Windows proxy enabled after closing MacroNest, dropping buffered TLS data, or hanging during shutdown.
- Fixed nested mouse action menus closing unexpectedly, opening in the wrong direction, or losing hover ownership.
- Fixed pinned Quick Action windows not being cleaned up correctly.
- Fixed Pin selection borders disappearing and the draw toolbar flashing during startup.
