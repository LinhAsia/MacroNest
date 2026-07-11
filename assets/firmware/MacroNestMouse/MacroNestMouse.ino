#include <Arduino.h>
#include <Mouse.h>

namespace {
constexpr uint8_t packetSize = 6;
uint8_t packet[packetSize];
uint8_t packetLength = 0;

uint8_t mouseButton(uint8_t button) {
  switch (button) {
    case 1: return MOUSE_LEFT;
    case 2: return MOUSE_RIGHT;
    case 3: return MOUSE_MIDDLE;
    default: return 0;
  }
}

void handlePacket() {
  switch (packet[1]) {
    case 1: {
      const int16_t x = static_cast<int16_t>((packet[2] << 8) | packet[3]);
      const int16_t y = static_cast<int16_t>((packet[4] << 8) | packet[5]);
      Mouse.move(static_cast<int8_t>(x), static_cast<int8_t>(y));
      break;
    }
    case 2: {
      const uint8_t button = mouseButton(packet[2]);
      if (button != 0) {
        if (packet[3]) Mouse.press(button); else Mouse.release(button);
      }
      break;
    }
    case 3:
      Mouse.move(0, 0, static_cast<int8_t>(packet[2]));
      break;
    case 0x7F:
      for (uint8_t i = 0; i < 5; ++i) {
        Mouse.move(20, 0);
        delay(20);
      }
      Mouse.click(MOUSE_LEFT);
      Serial.write(0xAC);
      break;
  }
}
}

void setup() {
  Mouse.begin();
  Serial.begin(115200);
  delay(3000);
  for (uint8_t i = 0; i < 10; ++i) {
    Mouse.move(20, 0);
    delay(30);
  }
}

void loop() {
  while (Serial.available()) {
    const uint8_t value = Serial.read();
    if (packetLength == 0 && value != 0xAA) continue;
    packet[packetLength++] = value;
    if (packetLength == packetSize) {
      handlePacket();
      packetLength = 0;
    }
  }
}
