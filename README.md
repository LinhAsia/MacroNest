<p align="left">
  <img src="assets/banner-v4.svg" alt="MacroNest Banner" width="100%" />
  <a href="https://github.com/NBaoLinh/MacroNest/stargazers"><img src="assets/star-button-v2.svg" alt="Star MacroNest" height="38" /></a>
  <a href="https://github.com/NBaoLinh/MacroNest/releases/latest"><img src="assets/download-button-v2.svg" alt="Download MacroNest" height="38" /></a>
  <a href="README_VI.md"><img src="assets/lang-vi-button-v2.svg" alt="Tiếng Việt" height="38" /></a>
</p>

## 🌟 Key Features

| Module | Description | Macro Integration |
| :--- | :--- | :--- |
| **⌨️ Macro Engine** | • Automate key sequences, mouse actions, loops, and conditions<br>• Centralized variable system with expression evaluation | *The central orchestrator of all automation and logic.* |
| **👁️ Computer Vision** | • Screen image detection (OpenCV Template Matching)<br>• Pixel & color change monitoring in custom regions<br>• Count pixels matching target colors in a region | *Trigger specific keystrokes or mouse clicks when visual targets or colors appear.* |
| **📝 OCR (Text Detection)** | • Windows Native OCR for ultra-low latency text recognition<br>• Match custom text, patterns, or numbers on screen | *Fire macros instantly when specific words or numbers are detected.* |
| **🪟 Window Controller** | • Resize and reposition windows with custom anchors or **Snap Layouts**<br>• **Live DWM Pinning**: Keep a cropped region of any window always on top<br>• Target specific windows, lock aspect ratio, and zoom views | *Focus target windows or adjust grid arrangements in real time.* |
| **🎙️ Audio Sensing** | • Monitor input levels and frequencies from system audio or microphone | *Execute macros the moment trigger volumes or pitches are reached.* |
| **🎵 Sound Effects** | • Trigger customizable audio alerts and trim custom sound clips | *Play sound alerts to signal macro completion or status errors.* |
| **➕ Custom Crosshair** | • Render center crosshairs (dot, cross, circle) with custom colors and opacity | *Toggle or swap crosshair overlays based on macro state.* |
| **📐 Geometry Overlay** | • Draw lines, boxes, circles, and shapes dynamically on the screen | *Highlight screen targets or draw bounding boxes dynamically.* |
| **🏷️ HUD Labels** | • Render custom text, active timers, and countdowns on the overlay | *Display variables value, progress, or execution steps.* |
| **📜 Script Command** | • Execute CMD and PowerShell commands directly | *Run system-level scripts inside macro sequences.* |
| **🖱️ Hardware Bypass** | • **Interception & Arduino**: Driver and hardware-level inputs for anti-cheat bypass<br>• **Mouse Path**: Record and replay smooth cursor trajectories | *Replay recorded mouse paths or adjust DPI sensitivity on the fly.* |
| **⚡ Quick Actions** | • Fast toggle for taskbar visibility, Windows key lock, and window topmost pinning<br>• Highlight active windows (rainbow/cyber border), screen draw tool, protractor, keyboard ASMR click sounds (Blue/Brown/Red switches), and keystroke mascot overlay | *Quick system utility tools accessible directly from the title bar.* |

---

## 🚀 Getting Started

### System Requirements

| Requirement | Minimum |
| :--- | :--- |
| **OS** | Windows 10 / 11 (64-bit) |
| **Runtime** | No installation required (portable `.exe`) |
| **Privileges** | Administrator (required to capture/inject inputs and manage windows globally) |

### Installation

1. Download **`MacroNest.exe`** from the [Latest Release](https://github.com/NBaoLinh/MacroNest/releases/latest).
2. Run the executable. No installer needed.

### Optional Dependencies (Downloaded from Settings)

- **OpenCV DLL**: Required for Computer Vision (Image Search).
- **Interception Driver**: Required for driver-level keyboard/mouse emulation.
- **Arduino Firmware**: Required for hardware-level input emulation via Arduino.
