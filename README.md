<p align="left">
  <a href="https://github.com/LinhAsia/MacroNest">
    <img src="assets/banner-v4.svg" alt="MacroNest Banner" width="100%" />
  </a>
  <a href="https://github.com/LinhAsia/MacroNest/stargazers"><img src="assets/star-button-v2.svg" alt="Star MacroNest" height="38" /></a>
  <a href="https://github.com/LinhAsia/MacroNest/releases/latest"><img src="assets/download-button-v2.svg" alt="Download MacroNest" height="38" /></a>
  <a href="README_vi.md"><img src="assets/lang-vi-button-v2.svg" alt="Tiếng Việt" height="38" /></a>
</p>

> **MacroNest is a free, open-source Windows desktop macro and automation tool.**
>
> Combine keyboard, mouse, OCR, image search, color detect, geometry drawing, pin part of a window, crosshair overlays, commands, sound playback, HUD labels, window control, and more in the same macro flow, with variables to build more flexible automation.

## Key Features

The modules below are built to work with the macro system, so you can combine them in the same macro flow.

| Module | What it does | How macros use it |
| :--- | :--- | :--- |
| **Macro Engine** | Run key presses, mouse actions, loops, waits, and conditional branches | Build logic: click buttons, enter input fields, loop tasks, and branch flow using variables |
| **Computer Vision** | Find images on screen, watch colors, and count matching pixels | Scan screen: locate game icons, wait for color changes, or trigger actions when an enemy bar fills up |
| **OCR** | Extract text/numbers from screen regions and save to variables, or check if specific text is found | Save scan results to variables (e.g. read coordinates/scores) and branch macro logic based on text matches |
| **Window Control** | Move, resize, layout (arrange in grid), pin, and zoom windows | Setup workspace: tile windows side-by-side, crop active regions, or center windows for precise clicking |
| **Audio Sense** | Monitor system audio, microphone levels, and audio frequency (pitch) | Handle sound cues: trigger automated responses when a voice call starts or game sound plays |
| **Sound Effects** | Play sound alerts, text-to-speech, and custom audio clips | Audible alerts: play warning sound on error, or read status messages aloud during execution |
| **Crosshair** | Render a customizable crosshair overlay on the screen | Visual aid: overlay a targeting crosshair for manual games, toggled on/off via macro steps |
| **Geometry Overlay** | Draw lines, rectangles, circles, and polygons on screen | Mark zones: highlight active scan regions, frame targets, or draw guides during macro execution |
| **HUD Labels** | Display customizable floating text labels and values on screen | Live dashboard: show variable values, current step status, or custom status labels overlaying the screen |
| **Timer** | Create stopwatches, countdowns, and cooldown overlays on screen | Track time: trigger actions after elapsed time, or overlay skill cooldown indicators on screen |
| **Script Command** | Execute CMD and PowerShell commands, capturing their output to variables | System integration: fetch system info, run Python scripts, or query network endpoints inside a macro |
| **Hardware Input** | Send input via Interception driver, Arduino boards, or recorded mouse paths | Bypass anti-cheat: send natural human-like input and replay recorded smooth mouse movements |

## Quick Actions

Quick Actions are small utility tools in the title bar. They are useful for fast manual access, but they are separate from the macro feature list above.

| Action | What it does |
| :--- | :--- |
| Taskbar | Hide or restore the Windows taskbar |
| Windows Key | Lock or unlock the Windows key |
| Window Pin | Keep a selected window always on top |
| Focus Highlight | Outline the active window with a configurable border and decoration |
| Protractor | Show a draggable protractor overlay for angle checking |
| Ruler | Measure the distance between two screen points and optionally copy the result |
| Get Coordinates | Pick a screen point and optionally copy X and Y |
| Get Color | Sample a screen color and optionally copy the hex value |
| Key Display | Show a live key overlay with normal and mascot display modes |
| Draw | Toggle the screen drawing overlay and configure its hotkey |
| Clear Overlays | Clear active geometry, HUD, and pin overlays from the screen |
| Key Sound | Play keyboard sound effects with selectable switch style and volume |

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
