import os

ui_path = 'src/ui.rs'
with open(ui_path, 'r', encoding='utf-8', newline='') as f:
    ui_text = f.read()

target = """    fn handle_image_search_capture_mouse_down(
        &mut self,
        ctx: &egui::Context,
        screen_x: i32,
        screen_y: i32,
    ) {
        if !self.vision_capture_active {
            return;
        }
        match self
            .vision_capture_mode
            .unwrap_or(VisionCaptureMode::Template)
        {
            VisionCaptureMode::ColorSample => {
                self.finish_image_search_color_pick_from_screen(ctx, screen_x, screen_y);
            }
            VisionCaptureMode::ColorPriorityAnchor => {
                self.finish_image_search_color_priority_anchor_pick_from_screen(
                    ctx, screen_x, screen_y,
                );
            }"""

replacement = """    fn handle_image_search_capture_mouse_down(
        &mut self,
        ctx: &egui::Context,
        screen_x: i32,
        screen_y: i32,
    ) {
        if !self.vision_capture_active {
            return;
        }
        match self
            .vision_capture_mode
            .unwrap_or(VisionCaptureMode::Template)
        {
            VisionCaptureMode::ColorSample | VisionCaptureMode::ColorPriorityAnchor => {
                // Do nothing on mouse down, wait for mouse up to capture!
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

ui_text, ok = safe_replace(ui_text, target, replacement)
if ok:
    print("Successfully replaced mouse down handler for color sample and priority anchor!")
else:
    print("Error: Could not find target mouse down handler block!")

with open(ui_path, 'w', encoding='utf-8', newline='') as f:
    f.write(ui_text)
