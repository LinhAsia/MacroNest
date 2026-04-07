# HUD Investigation Notes

This file tracks HUD approaches that were tried and did not fix the edge/video clipping issue.

## Reproduced symptom

- HUD text is visible in MacroNest and on the desktop.
- On some apps, especially Edge/YouTube-like video surfaces, part of the HUD disappears or falls behind the video surface.
- Hovering video timeline / thumbnail preview changes the HUD visibility state.

## Do not retry

- Changing `WS_EX_TRANSPARENT` alone.
- Adding or removing `WS_EX_NOREDIRECTIONBITMAP`.
- Forcing `SetWindowPos(HWND_TOPMOST)` before every HUD paint.
- Adding `DwmFlush()` after layered paints.
- Binding HUD ownership to the foreground/maximized window.
- Making HUD dependent on UI focus / UI foreground state.
- Changing HUD auto-hide flags alone.
- Clamping HUD coordinates to a single monitor as a fix for the bug.
- Repainting more often or forcing repaint on every refresh.

## What seems more likely

- A compositor / overlay-plane issue with some browser/video surfaces.
- The desktop overlay path is not reliable enough for a 100% guarantee on every monitor/app combination.
- If a guaranteed overlay is needed for video, the more reliable directions are:
  - render inside the target app surface
  - or browser-side overlay for Edge/YouTube

## Keep in mind

- Crosshair can still work while HUD fails because the two paths are not identical.
- The HUD issue is not just text rendering. It behaves like a window/compositor stacking problem.
