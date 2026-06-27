<p align="left">
  <a href="https://github.com/LinhAsia/MacroNest">
    <img src="assets/banner-v4.svg" alt="MacroNest Banner" width="100%" />
  </a>
  <a href="https://github.com/LinhAsia/MacroNest/stargazers"><img src="assets/star-button-v2.svg" alt="Star MacroNest" height="38" /></a>
  <a href="https://github.com/LinhAsia/MacroNest/releases/latest"><img src="assets/download-button-v2.svg" alt="Download MacroNest" height="38" /></a>
</p>

> **MacroNest is a Windows desktop macro tool.**
>
> Combine keyboard, mouse, OCR, image search, color detect, geometry drawing, pin part of a window, crosshair overlays, commands, sound playback, HUD labels, window control, and more in the same macro flow, with variables to build more flexible automation.

## Key Features

The modules below are built to work with the macro system, so you can combine them in the same macro flow.

| Module | What it does | How macros use it |
| :--- | :--- | :--- |
| Macro Engine | Run key presses, mouse actions, loops, waits, and conditions | Build the main flow of your automation |
| Computer Vision | Find images on screen, watch colors, and count matching pixels | Trigger actions when something appears on screen |
| OCR | Read text and numbers from the screen with fast local PaddleOCR | Run a macro when text matches |
| Window Control | Move, resize, pin, and zoom windows | Control your workspace during a macro |
| Audio Sense | Watch system audio or microphone levels and pitch | Trigger actions from sound |
| Sound Effects | Play sound alerts and use custom clips | Confirm status, warnings, or macro events |
| Crosshair | Show a custom crosshair with your own style | Turn overlays on or off with macros |
| Geometry Overlay | Draw lines, boxes, circles, and other shapes | Mark screen targets during a macro |
| HUD Labels | Show text, timers, and countdowns on screen | Display values and progress while running |
| Script Command | Run CMD and PowerShell commands | Call system scripts inside a macro |
| Hardware Input | Use Interception, Arduino, and recorded mouse paths | Send input in different ways when needed |

## Quick Actions

Quick Actions are small utility tools in the title bar. They are useful for fast manual access, but they are separate from the macro feature list above.

- Toggle taskbar visibility
- Lock or unlock the Windows key
- Pin a window on top
- Highlight the active window border
- Use the protractor overlay
- Get screen coordinates
- Pick colors from the screen
- Show key display overlay
- Draw on the screen and capture from it
- Enable key sound effects

## Getting Started

### System Requirements

| Requirement | Minimum |
| :--- | :--- |
| OS | Windows 10 / 11 (64-bit) |
| Runtime | No install needed, portable `.exe` |
| Privileges | Administrator access |

### Installation

1. Download **`MacroNest.exe`** from the [latest release](https://github.com/LinhAsia/MacroNest/releases/latest).
2. Run the file.

### Optional Downloads

These can be downloaded from the app settings:

- OpenCV DLL for image search
- Interception driver for low-level keyboard and mouse input
- Arduino firmware for hardware input emulation

## License

Released under the MIT License. See [LICENSE](LICENSE).
