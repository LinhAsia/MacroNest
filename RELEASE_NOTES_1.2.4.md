# MacroNest 1.2.4

## New features

- Added dynamic variable names and the `charat()` expression function.
- Added support for assigning and comparing whitespace or special characters.
- Added App Data size information and Windows startup options.
- Added panning for zoomed Pin and Source Crop previews, plus opacity control for Pin presets.
- Added Focus Mode and a Window Opacity Quick Action.
- Added a Quick Video Recorder for full-screen, window, or region capture with configurable FPS and system audio.
- Added a Video Library with playback, thumbnails, trimming, compression, and file actions.
- Added `ReadMemory` and `WriteMemory` macro actions.
- Added a complete Memory workspace with scanning, saved addresses, pointer search, freezing, structure and region views, disassembly, debugging, and x86/x64 support.
- Added a Network recorder for HTTP, HTTPS, and WebSocket traffic, with filtering, request details, and pinned monitoring.
- Added optional Frida-based Network tracing through Downloaded Tools.
- Added Copy and Paste controls to preset cards across Commands, Window Control, Pin, Mouse, Vision, AudioSense, OCR, Geometry, Media, HUD, and Timer.

## Improvements

- Improved Gemini response speed with web search support.
- Reduced the main executable from approximately 107 MB to 42 MB.
- Improved Memory scanning, process selection, pointer workflows, tables, and debugger reliability.
- Improved Network capture reliability and automatic proxy restoration.
- Improved video recording startup, audio capture, seeking, trimming, and library responsiveness.
- Added per-step Mouse Click Delay and made Memory hotkeys use the same global trigger path as macro hotkeys.
- Imported macro groups now stay inside the currently open folder.
- Preserved Crosshair preset positions when saving and reopening the app.
- Unified macro-step and Quick Action typography.
- Added expression highlighting for SetVariable fields and expression evaluation inside HUD text.
- Made HUD changes appear immediately.
- Expanded Vietnamese localization across Memory, Vision, Downloaded Tools, popup title bars, and context-menu actions.

## Bug fixes

- Fixed TypeText Paste mode not saving or working correctly.
- Fixed zero-delay infinite macro loops causing UI hangs.
- Fixed macro trigger keys occasionally failing immediately after switching to another window.
- Fixed per-step mouse click delay not being honored correctly by click actions.
- Fixed nested mouse action menus closing or opening incorrectly.
- Fixed pinned Quick Action windows not being cleaned up correctly.
- Fixed disappearing Pin selection borders and draw-toolbar flashing.
