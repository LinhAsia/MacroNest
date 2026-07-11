#include <Mouse.h>
#include <HID.h>

namespace {
constexpr uint8_t kPacketSize = 6;
uint8_t packet[kPacketSize];
uint8_t packetLength = 0;
uint8_t buttons = 0;

bool sendMouseReport(int8_t dx, int8_t dy, int8_t wheel) {
  const uint8_t report[4] = {buttons, static_cast<uint8_t>(dx),
                             static_cast<uint8_t>(dy), static_cast<uint8_t>(wheel)};
  return HID().SendReport(1, report, sizeof(report)) == 5;
}

void handlePacket() {
  switch (packet[1]) {
    case 1: {
      const int16_t dx = static_cast<int16_t>((packet[2] << 8) | packet[3]);
      const int16_t dy = static_cast<int16_t>((packet[4] << 8) | packet[5]);
      sendMouseReport(static_cast<int8_t>(dx), static_cast<int8_t>(dy), 0);
      break;
    }
    case 2: {
      uint8_t button = 0;
      if (packet[2] == 1) button = MOUSE_LEFT;
      if (packet[2] == 2) button = MOUSE_RIGHT;
      if (packet[2] == 3) button = MOUSE_MIDDLE;
      if (button != 0) {
        if (packet[3] != 0) buttons |= button;
        else buttons &= ~button;
        sendMouseReport(0, 0, 0);
      }
      break;
    }
    case 3:
      sendMouseReport(0, 0, static_cast<int8_t>(packet[2]));
      break;
    case 0x7F: {
      bool sent = true;
      for (uint8_t i = 0; i < 5; ++i) {
        sent &= sendMouseReport(20, 0, 0);
        delay(20);
      }
      buttons = MOUSE_LEFT;
      sent &= sendMouseReport(0, 0, 0);
      delay(20);
      buttons = 0;
      sent &= sendMouseReport(0, 0, 0);
      Serial.write(sent ? 0xAC : 0xE1);
      break;
    }
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
