import os

overlay_path = 'src/overlay.rs'
with open(overlay_path, 'r', encoding='utf-8', newline='') as f:
    text = f.read()

target = """            // 2. If MacroNest UI is in the foreground, bypass all mouse events.
            if UI_WINDOW_FOREGROUND.load(Ordering::Relaxed) {
                return CallNextHookEx(None, code, wparam, lparam);
            }

            // 3. For actual click/wheel events (extremely rare), check if the physical click target
            // is actually the MacroNest window. This ensures that clicks on game windows that cover/obscure
            // MacroNest in the background are NOT bypassed, allowing macro triggering to work perfectly!
            let hwnd_at_point = WindowFromPoint(info.pt);
            if !hwnd_at_point.0.is_null() {
                let root = GetAncestor(hwnd_at_point, GA_ROOT);
                if !root.0.is_null() && window_belongs_to_current_process(root) {
                    return CallNextHookEx(None, code, wparam, lparam);
                }
            }"""

replacement = """            // 2. If MacroNest UI is in the foreground, bypass all mouse events.
            if UI_WINDOW_FOREGROUND.load(Ordering::Relaxed) && !is_vision_capture_mouse_blocked() {
                return CallNextHookEx(None, code, wparam, lparam);
            }

            // 3. For actual click/wheel events (extremely rare), check if the physical click target
            // is actually the MacroNest window. This ensures that clicks on game windows that cover/obscure
            // MacroNest in the background are NOT bypassed, allowing macro triggering to work perfectly!
            let hwnd_at_point = WindowFromPoint(info.pt);
            if !hwnd_at_point.0.is_null() && !is_vision_capture_mouse_blocked() {
                let root = GetAncestor(hwnd_at_point, GA_ROOT);
                if !root.0.is_null() && window_belongs_to_current_process(root) {
                    return CallNextHookEx(None, code, wparam, lparam);
                }
            }"""

def safe_replace(text, target, replacement):
    if target in text:
        return text.replace(target, replacement), True
    target_lf = target.replace('\r\n', '\n')
    text_lf = text.replace('\r\n', '\n')
    if target_lf in text_lf:
        text_lf = text_lf.replace(target_lf, replacement.replace('\r\n', '\n'))
        if '\r\n' in text:
            return text_lf.replace('\n', '\r\n'), True
        else:
            return text_lf, True
    return text, False

text, ok = safe_replace(text, target, replacement)
if ok:
    print("Successfully replaced foreground bypass handlers in src/overlay.rs!")
else:
    print("Error: Could not find target foreground bypass block!")

with open(overlay_path, 'w', encoding='utf-8', newline='') as f:
    f.write(text)
