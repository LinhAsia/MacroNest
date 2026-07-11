#include <Mouse.h>

namespace {
constexpr uint8_t kPacketSize = 6;
uint8_t packet[kPacketSize];
uint8_t packetLength = 0;

void handlePacket() {
  switch (packet[1]) {
    case 1: {
      const int16_t dx = static_cast<int16_t>((packet[2] << 8) | packet[3]);
      const int16_t dy = static_cast<int16_t>((packet[4] << 8) | packet[5]);
      Mouse.move(static_cast<int8_t>(dx), static_cast<int8_t>(dy), 0);
      break;
    }
    case 2: {
      uint8_t button = 0;
      if (packet[2] == 1) button = MOUSE_LEFT;
      if (packet[2] == 2) button = MOUSE_RIGHT;
      if (packet[2] == 3) button = MOUSE_MIDDLE;
      if (button != 0) {
        if (packet[3] != 0) Mouse.press(button);
        else Mouse.release(button);
      }
      break;
    }
    case 3:
      Mouse.move(0, 0, static_cast<int8_t>(packet[2]));
      break;
  }
}
}

void setup() {
  Serial.begin(115200);
  Mouse.begin();
}

void loop() {
  while (Serial.available() > 0) {
    const uint8_t value = static_cast<uint8_t>(Serial.read());
    if (packetLength == 0 && value != 0xAA) continue;
    packet[packetLength++] = value;
    if (packetLength == kPacketSize) {
      handlePacket();
      packetLength = 0;
    }
  }
}
