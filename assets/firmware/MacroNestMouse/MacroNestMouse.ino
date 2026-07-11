#include <Arduino.h>
#include <HID.h>
#include <Mouse.h>

namespace {
constexpr uint8_t packetSize = 6;
uint8_t packet[packetSize];
uint8_t packetLength = 0;

bool sendMouseReport(uint8_t buttons, int8_t x, int8_t y, int8_t wheel = 0) {
  const uint8_t report[4] = {buttons, static_cast<uint8_t>(x),
                             static_cast<uint8_t>(y), static_cast<uint8_t>(wheel)};
  return HID().SendReport(1, report, sizeof(report)) == sizeof(report) + 1;
}

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
      sendMouseReport(0, static_cast<int8_t>(x), static_cast<int8_t>(y));
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
    case 0x7F: {
      bool sent = true;
      for (uint8_t i = 0; i < 5; ++i) {
        sent &= sendMouseReport(0, 20, 0);
        delay(20);
      }
      sent &= sendMouseReport(MOUSE_LEFT, 0, 0);
      sent &= sendMouseReport(0, 0, 0);
      Serial.write(sent ? 0xAC : 0xE1);
      break;
    }
    case 0x7E:
      Serial.write(0xA5);
      break;
  }
}
}

void setup() {
  Mouse.begin();
  Serial.begin(115200);
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
