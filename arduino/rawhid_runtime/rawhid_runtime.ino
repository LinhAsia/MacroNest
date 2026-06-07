#include <HID-Project.h>

namespace {
constexpr uint8_t kMagic = 0xA5;
constexpr uint8_t kEnd = 0x5A;
constexpr uint8_t kCmdMove = 0x01;
constexpr uint8_t kCmdButton = 0x02;
constexpr uint8_t kCmdWheel = 0x03;
constexpr uint8_t kBtnLeft = 0x01;
constexpr uint8_t kBtnRight = 0x02;
constexpr uint8_t kBtnMiddle = 0x04;
constexpr size_t kFrameSize = 8;

uint8_t g_rawhid_buffer[64];
uint8_t g_frame[kFrameSize];
uint8_t g_frame_index = 0;
uint8_t g_buttons = 0;

uint8_t button_mask_from_id(uint8_t button_id) {
  switch (button_id) {
    case 1:
      return kBtnLeft;
    case 2:
      return kBtnRight;
    case 3:
      return kBtnMiddle;
    default:
      return 0;
  }
}

uint8_t mouse_api_button_from_mask(uint8_t mask) {
  switch (mask) {
    case kBtnLeft:
      return MOUSE_LEFT;
    case kBtnRight:
      return MOUSE_RIGHT;
    case kBtnMiddle:
      return MOUSE_MIDDLE;
    default:
      return 0;
  }
}

void move_chunked(int16_t dx, int16_t dy) {
  while (dx != 0 || dy != 0) {
    int8_t step_x = 0;
    int8_t step_y = 0;

    if (dx > 127) {
      step_x = 127;
    } else if (dx < -127) {
      step_x = -127;
    } else {
      step_x = static_cast<int8_t>(dx);
    }

    if (dy > 127) {
      step_y = 127;
    } else if (dy < -127) {
      step_y = -127;
    } else {
      step_y = static_cast<int8_t>(dy);
    }

    Mouse.move(step_x, step_y, 0);
    dx -= step_x;
    dy -= step_y;
  }
}

void handle_button_command(uint8_t button_id, uint8_t state) {
  const uint8_t mask = button_mask_from_id(button_id);
  if (mask == 0) {
    return;
  }

  const uint8_t mouse_button = mouse_api_button_from_mask(mask);
  if (mouse_button == 0) {
    return;
  }

  if (state) {
    if ((g_buttons & mask) == 0) {
      Mouse.press(mouse_button);
      g_buttons |= mask;
    }
  } else if (g_buttons & mask) {
    Mouse.release(mouse_button);
    g_buttons &= static_cast<uint8_t>(~mask);
  }
}

void handle_frame(const uint8_t* frame) {
  const uint8_t cmd = frame[1];
  if (cmd == kCmdMove) {
    const int16_t dx =
        static_cast<int16_t>((static_cast<int16_t>(frame[2]) << 8) | frame[3]);
    const int16_t dy =
        static_cast<int16_t>((static_cast<int16_t>(frame[4]) << 8) | frame[5]);
    move_chunked(dx, dy);
    return;
  }

  if (cmd == kCmdButton) {
    handle_button_command(frame[2], frame[3]);
    return;
  }

  if (cmd == kCmdWheel) {
    const int8_t wheel = static_cast<int8_t>(frame[2]);
    Mouse.move(0, 0, wheel);
  }
}

void consume_byte(uint8_t value) {
  if (g_frame_index == 0) {
    if (value != kMagic) {
      return;
    }
    g_frame[g_frame_index++] = value;
    return;
  }

  g_frame[g_frame_index++] = value;
  if (g_frame_index < kFrameSize) {
    return;
  }

  if (g_frame[0] == kMagic && g_frame[kFrameSize - 1] == kEnd) {
    handle_frame(g_frame);
  }
  g_frame_index = 0;
}
}  // namespace

void setup() {
  Mouse.begin();
  RawHID.begin(g_rawhid_buffer, sizeof(g_rawhid_buffer));
}

void loop() {
  int available = RawHID.available();
  while (available-- > 0) {
    const int value = RawHID.read();
    if (value >= 0) {
      consume_byte(static_cast<uint8_t>(value));
    }
  }
}
