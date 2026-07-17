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
| **OCR** | Extract text/numbers from screen regions and check if specific text is found | Save scan results to variables (e.g. read coordinates/scores) and branch macro logic based on text matches |
| **Window Control** | Move, resize, layout (arrange in grid), pin, and zoom windows | Setup workspace: tile windows side-by-side, crop active regions, or center windows for precise clicking |
| **Audio Sense** | Monitor system audio, microphone levels, and audio frequency (pitch) | Handle sound cues: trigger automated responses when a voice call starts or game sound plays |
| **Sound Effects** | Play sound alerts, text-to-speech, and custom audio clips | Audible alerts: play warning sound on error, or read status messages aloud during execution |
| **Crosshair** | Render a customizable crosshair overlay on the screen | Visual aid: overlay a targeting crosshair for manual games, toggled on/off via macro steps |
| **Geometry Overlay** | Draw shapes (Points, Lines, Circles, Rectangles, Ellipses, Arrows, Polylines, Polygons, Arcs, Labels, and SVGs) on screen | Draw dynamic visual indicators, track zones, or display text labels on screen using mathematical expressions bound to variables |
| **HUD Labels** | Display customizable floating text labels and values on screen | Live dashboard: show variable values, current step status, or custom status labels overlaying the screen |
| **Timer** | Create stopwatches, countdowns, and cooldown overlays on screen | Track time: read stopwatch/countdown values directly into macro variables, trigger actions after elapsed time, or overlay skill cooldown indicators on screen |
| **Script Command** | Execute CMD and PowerShell commands, capturing their output to variables | System integration: execute local scripts, run command-line tools, or query network endpoints inside a macro |
| **Hardware Input** | Send input via Interception driver, Arduino boards, or recorded mouse paths | Emulate input: send low-level mouse and keyboard signals to ensure compatibility with high-security applications or games |

## Quick Actions

Quick Actions are small utility tools in the title bar. They are useful for fast manual access, but they are separate from the macro feature list above.

| Action | What it does |
| :--- | :--- |
| Taskbar | Hide or restore the Windows taskbar |
| Windows Key | Lock or unlock the Windows key |
| Window Pin | Keep a selected window always on top |
| Focus Highlight | Outline the active window with a configurable border and decoration |
| Focus Mode | Dim everything outside the focused or selected window |
| Window Opacity | Change the selected window's native opacity directly from 0% to 100% |
| Protractor | Show a draggable protractor overlay for angle checking |
| Ruler | Measure the distance between two screen points and optionally copy the result |
| Get Coordinates | Pick a screen point and optionally copy X and Y |
| Get Color | Sample a screen color and optionally copy the hex value |
| Key Display | Show a live key overlay with normal and mascot display modes |
| Draw | Toggle the screen drawing overlay and configure its hotkey |
| Screen Recorder | Record the full screen, focused window, selected window, or a selected region at 60 FPS using Windows Graphics Capture. Start and stop with a trigger key; videos are saved as MP4 |
| Clear Overlays | Clear active geometry, HUD, and pin overlays from the screen |
| Key Sound | Play keyboard sound effects with selectable switch style and volume |

## Expression Help

<details>
  <summary>Expand expression syntax and examples</summary>

### Operators

| Syntax | Meaning | Example | Result |
| :--- | :--- | :--- | :--- |
| `a + b` | Add | `2 + 3` | `5` |
| `a - b` | Subtract | `10 - 4` | `6` |
| `a * b` | Multiply | `3 * 4` | `12` |
| `a / b` | Divide | `5 / 2` | `2.5` |
| `a ^ b` | Power | `5^2` | `25` |
| `a == b` | Equal comparison | `5 == 5` | `1` |
| `a != b` | Not-equal comparison | `5 != 5` | `0` |
| `a > b` / `a >= b` | Greater-than comparison | `8 >= 3` | `1` |
| `a < b` / `a <= b` | Less-than comparison | `2 < 1` | `0` |

### Constants

| Syntax | Meaning | Example | Result |
| :--- | :--- | :--- | :--- |
| `pi` | Pi constant | `degrees(pi)` | `180` |
| `e` | Euler's number | `round(e, 3)` | `2.718` |

### Core Functions

| Function | Meaning | Example | Result |
| :--- | :--- | :--- | :--- |
| `random(min, max)` | Random integer in range | `random(10, 20)` | `10..20` |
| `choice(a, b, ...)` | Pick one value at random (supports numbers, text, or a mix) | 1. `choice(10, 20, 30)` (numbers)<br>2. `choice(apple, banana, cherry)` (text)<br>3. `choice(Level: 5, 50, critical)` (mixed) | 1. `10` or `20` or `30`<br>2. `apple` or `banana` or `cherry`<br>3. `Level: 5` or `50` or `critical` |
| `clamp(x, min, max)` | Keep `x` inside a range | `clamp(120, 0, 100)` | `100` |
| `between(x, a, b)` | Check whether `x` is inside a range (inclusive) | `between(7, 1, 10)` | `1` |
| `min(a, b)` | Smaller value | `min(20, 50)` | `20` |
| `max(a, b)` | Larger value | `max(20, 50)` | `50` |
| `abs(a)` | Absolute value | `abs(-50)` | `50` |
| `div(a, b)` | Integer division using truncation | `div(5, 2)` | `2` |
| `mod(a, b)` | Remainder | `mod(5, 2)` | `1` |
| `round(a, digits)` | Round to digits | `round(863.6897, 2)` | `863.69` |
| `ceil(a)` | Round up | `ceil(pi)` | `4` |
| `floor(a)` | Round down | `floor(pi)` | `3` |
| `sqrt(a)` | Square root | `sqrt(9)` | `3` |
| `pow(a, b)` | Power function | `pow(2, 3)` | `8` |
| `factorial(n)` | Factorial | `factorial(5)` | `120` |
| `gcd(a, b, ...)` | Greatest common divisor | `gcd(24, 36, 48)` | `12` |
| `lcm(a, b, ...)` | Least common multiple | `lcm(4, 6, 8)` | `24` |
| `isqrt(n)` | Integer square root | `isqrt(17)` | `4` |
| `comb(n, k)` | Combination | `comb(5, 2)` | `10` |
| `perm(n, k)` | Permutation | `perm(5, 2)` | `20` |

### Trigonometry and Angles

| Function | Meaning | Example | Result |
| :--- | :--- | :--- | :--- |
| `sin(a)` | Sine | `sin(radians(30)) * 1000` | `500` |
| `cos(a)` | Cosine | `cos(radians(60)) * 1000` | `500` |
| `tan(a)` | Tangent | `tan(45)` | depends on input unit |
| `asin(a)` | Arc sine | `asin(0.5)` | angle in radians |
| `acos(a)` | Arc cosine | `acos(0.5)` | angle in radians |
| `atan(a)` | Arc tangent | `degrees(atan(1))` | `45` |
| `atan2(y, x)` | 2-argument arc tangent | `degrees(atan2(1, 1))` | `45` |
| `sinh(a)` | Hyperbolic sine | `sinh(1)` | numeric result |
| `cosh(a)` | Hyperbolic cosine | `cosh(1)` | numeric result |
| `tanh(a)` | Hyperbolic tangent | `tanh(1)` | numeric result |
| `degrees(rad)` | Radians to degrees | `degrees(pi)` | `180` |
| `radians(deg)` | Degrees to radians | `radians(180)` | about `3.14159` |

### Logarithms and Exponents

| Function | Meaning | Example | Result |
| :--- | :--- | :--- | :--- |
| `ln(a)` | Natural log | `ln(e)` | `1` |
| `log(a)` | Natural log | `log(e)` | `1` |
| `log10(a)` | Base-10 log | `log10(1000)` | `3` |
| `exp(a)` | `e^a` | `exp(1)` | about `2.71828` |

### Text Helpers

| Function | Meaning | Example | Result |
| :--- | :--- | :--- | :--- |
| `contains(a, b)` | Check whether text `a` contains text `b` (supports numbers, text, or a mix) | 1. `contains(hello, world)` (text)<br>2. `contains(Gold: 5000, 5000)` (mixed)<br>3. `contains(12345, 99)` (numbers) | 1. `0` (false)<br>2. `1` (true)<br>3. `0` (false) |
| `concat(a, b, ...)` | Join multiple values into one text result | `concat(Player, "-", 01)` | `Player-01` |
| `substr(text, start, len)` | Get part of a string (supports numbers, text, or a mix) | 1. `substr(banana, 2, 3)` (text)<br>2. `substr(Rank #1: Player, 9, 6)` (mixed)<br>3. `substr(123456, 1, 4)` (numbers) | 1. `nan`<br>2. `Player`<br>3. `2345` |
| `charat(text, index)` | Get one character at a zero-based position; returns empty text when out of range | `charat("hello", 1)` | `e` |
| `len(text)` | Count characters (supports numbers, text, or a mix) | 1. `len(apple)` (text)<br>2. `len(Score: 9999)` (mixed)<br>3. `len(453454)` (numbers) | 1. `5`<br>2. `11`<br>3. `6` |
| `lower(text)` | Convert text to lowercase | `lower(HeLLo)` | `hello` |
| `upper(text)` | Convert text to uppercase | `upper(HeLLo)` | `HELLO` |
| `trim(text)` | Remove leading and trailing spaces | `trim("  hello  ")` | `hello` |
| `myVar.toNumber` | Extract digits from a text variable and convert them to a number (ignores non-digits) | If variable `A` is `"Gold: 500"` (text):<br>`A.toNumber` | `500` (numeric) |
| `myVar.toString` | Convert a variable to text by filtering out all digits (keeps only non-digits) | If variable `A` is `"Wave #10"` (text):<br>`A.toString` | `"Wave #"` (text) |

Text comparisons preserve quoted whitespace and symbols. For example, `charat(a, 0) == " "` returns `1` when the first character is a space.

Variable names support computed `{}` parts. Setting `item[{i}]` while `i = 3` writes `item[3]`; `item[{len(text)}]` works the same way. This can be used with a loop to create `item[1]`, `item[2]`, and so on.

### Built-in Variables (Numeric)

| Variable | Meaning | Example / Notes |
| :--- | :--- | :--- |
| `screen.width` | Width of the primary screen in pixels | `screen.width` |
| `screen.height` | Height of the primary screen in pixels | `screen.height` |
| `mouse.x` | Current X coordinate of the mouse | `mouse.x` |
| `mouse.y` | Current Y coordinate of the mouse | `mouse.y` |
| `mouse.sensitivity` | Current system mouse sensitivity speed | `mouse.sensitivity` |
| `volume.level` | Current system volume level (0 to 100) | `volume.level` |
| `window.x` | X coordinate of target window's left edge | `window.x` |
| `window.y` | Y coordinate of target window's top edge | `window.y` |
| `window.right` | X coordinate of target window's right edge | `window.right` |
| `window.bottom` | Y coordinate of target window's bottom edge | `window.bottom` |
| `window.width` | Width of the target window | `window.width` |
| `window.height` | Height of the target window | `window.height` |
| `window.centerX` | X coordinate of the target window's center | `window.centerX` |
| `window.centerY` | Y coordinate of the target window's center | `window.centerY` |

### Built-in Variables (System and Text)

| Variable / Property | Meaning | Example / Notes |
| :--- | :--- | :--- |
| `system.year` / `month` / `day` | Current calendar year, month, or day | `system.year` |
| `system.hour` / `minute` / `second` | Current system time components | `system.hour` |
| `system.millisecond` | Current system millisecond | `system.millisecond` |
| `system.date` | Current system date | e.g. `2026-07-09` |
| `system.time` | Current system time | e.g. `04:24:00` |
| `window.title` | Title text of the target window | `window.title` |
| `clipboard.text` | Current text content in the clipboard | `clipboard.text` |

### Notes

- Expression fields evaluate variables and functions directly.
- Text fields keep plain text as-is; use `{...}` to inject variables or math into text.
- Comparison operators return `1` for true and `0` for false.
- Some macro fields store final values as integers, so decimal results may be rounded there.
- Any math evaluation errors or division by zero (e.g. `5/0`) will return `0`.

</details>

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
- OCR data files for text recognition (OCR)

## License

Released under the MIT License. See [LICENSE](LICENSE).
