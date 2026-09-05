# MacroNest v1.2.7

## New features

- **Multi-Threaded SIMD Memory Scanner**: Ultra-fast memory scanning powered by AVX2/SSE4.1 instructions and pre-allocated thread buffers.
- **Cheat Engine-Style Snapshot Architecture**: Instant sub-100ms unknown value scans with parallel snapshot filtering.
- **Automatic AOB Pattern Detection**: Paste hex strings or AOB patterns directly; MacroNest auto-detects and switches scan types seamlessly.
- **2-Sample AOB Signature Generator**: Generate robust AOB signatures with wildcards (`?`) across two memory samples and relocate addresses with one click.
- **Manual AOB Compare Dialog**: Inspect and compare memory samples across multiple addresses with sample chaining and 64/128-byte copy actions.
- **Numeric Value Type Resolution**: Automatic interpretation and decoding of raw memory results according to selected data types (Byte, Int16, Int32, Int64, Float, Double).
- **First-Class "Set as Base" Action**: Added `Set as Base` across scan results, stable pointer dialog, and deep pointer dialog, complete with macro action and hotkey support.
- **Multi-Round Stable Pointer Validation**: Added multi-round verification filter to retain only proven pointers across restarts and memory relocations.
- **OBS Game Capture Integration**: Direct graphics-hook capture for both 32-bit and 64-bit DirectX/OpenGL games via shared GPU swapchain textures.
- **Direct3D 11 NVIDIA NVENC Zero-Copy Encoder**: Native NVENC hardware encoder delivering rock-solid 60 FPS recording with minimal CPU and GPU overhead.
- **OBS-Style Windows Graphics Capture (WGC)**: Direct window streaming capture without stealing window focus.
- **Interactive Window Pin Overlay**: Pin windows directly from interactive, clickable title bar badges with occlusion culling and quick-action toggles.
- **Enhanced Macro Recording**: Direct `MoveAbs` coordinate recording, explicit `MouseDown`/`MouseUp` drag events, and modifier key press capture.
- **Mouse Path Preview Dismissal**: Added an explicit `Clear preview` button, Spacebar toggle, and automatic overlay dismissal on tab switch or focus loss.

## Improvements

- **GPU DirectComposition Overlay**: Unified overlay rendering with DirectComposition for crisp, zero-latency drawing.
- **144Hz+ High-Precision Timing**: Enabled 1ms high-precision system timer, zero VSync latency, and instant overlay tracking.
- **Eliminated Window Capture Lag**: Triple-buffered staging textures, 16MB pipe buffer, clock-aligned frame pacing, and WASAPI silence compensation.
- **Cached Graphics Offsets & Lazy Device Init**: Eliminated game capture preparation stutter.
- **Dynamic Taskbar Recording Badge**: Live red badge overlay on the taskbar icon during recording without interfering with main application icons.
- **Deep Pointer Scan Performance**: Optimized with fast range caching, binary module search, parallel breadth-first search, and batch validation.
- **Virtualized UI Tables**: Virtualized memory address lists and candidate tables, eliminating scrolling and rendering lag.
- **Process Handle Caching**: Cached process handles on `WriteMemory` and added support for `module+offset` syntax.
- **Smooth Direct2D Screen Drawing**: Enhanced freehand screen drawing with Direct2D path geometry, rounded line caps, and quadratic bezier curves.
- **Instant Windows Startup**: Upgraded startup to Windows Task Scheduler with Highest Privileges, bypassing UAC confirmation prompts and startup delays.
- **Reduced RAM Footprint**: Automatic working set memory trimming after startup warmup and upon minimizing to tray.

## Bug fixes

- Fixed game and application crashes when opening code lists during active debugging watch; throttled event updates safely.
- Fixed ESP camera matrix coordinate projection skew, inverted matrix math, and duplicate candidate boxes.
- Fixed target window resetting to "Any focused window" when the configured target application is not running.
- Fixed mouse stutter on overlay tracking by switching overlay windows to `WS_EX_TOOLWINDOW`.
- Fixed memory scan hotkeys failing when modifier keys were held.
- Fixed double cursor and window hiding issues during screen coordinate picking.
- Fixed address list double-click value editing and restored target process auto-attachment on app restart.
- Fixed overlay hint border and yellow recording indicator clipping during window captures.
- Fixed MoveAbs and IfStart PixelColor cursor hit-testing and screen coordinate hints.
