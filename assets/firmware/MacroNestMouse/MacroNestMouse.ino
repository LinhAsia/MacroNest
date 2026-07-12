#include <Arduino.h>
#include <HID.h>
#include <Mouse.h>

namespace {
constexpr uint8_t kAbsoluteMouseReportId = 4;
const uint8_t kAbsoluteMouseDescriptor[] PROGMEM = {
  0x05, 0x01,        // Usage Page (Generic Desktop)
  0x09, 0x02,        // Usage (Mouse)
  0xA1, 0x01,        // Collection (Application)
  0x85, kAbsoluteMouseReportId,
  0x09, 0x01,        // Usage (Pointer)
  0xA1, 0x00,        // Collection (Physical)
  0x05, 0x09,        // Usage Page (Button)
  0x19, 0x01,        // Usage Minimum (1)
  0x29, 0x03,        // Usage Maximum (3)
  0x15, 0x00,        // Logical Minimum (0)
  0x25, 0x01,        // Logical Maximum (1)
  0x95, 0x03,        // Report Count (3)
  0x75, 0x01,        // Report Size (1)
  0x81, 0x02,        // Input (Data, Variable, Absolute)
  0x95, 0x01,        // Report Count (1)
  0x75, 0x05,        // Report Size (5)
  0x81, 0x03,        // Input (Constant, Variable, Absolute)
  0x05, 0x01,        // Usage Page (Generic Desktop)
  0x09, 0x30,        // Usage (X)
  0x09, 0x31,        // Usage (Y)
  0x16, 0x00, 0x00,  // Logical Minimum (0)
  0x26, 0xFF, 0x7F,  // Logical Maximum (32767)
  0x75, 0x10,        // Report Size (16)
  0x95, 0x02,        // Report Count (2)
  0x81, 0x02,        // Input (Data, Variable, Absolute)
  0xC0,
  0xC0
};
HIDSubDescriptor absoluteMouseNode(kAbsoluteMouseDescriptor, sizeof(kAbsoluteMouseDescriptor));
struct AbsoluteMouseRegistration {
  AbsoluteMouseRegistration() { HID().AppendDescriptor(&absoluteMouseNode); }
} absoluteMouseRegistration;

constexpr uint8_t kPacketSize = 8;
constexpr uint8_t kHeader = 0xA5;
constexpr uint8_t kReply = 0x5A;
uint8_t packet[kPacketSize];
uint8_t packetLength = 0;
uint16_t absoluteX = 0;
uint16_t absoluteY = 0;
uint8_t absoluteButtons = 0;
bool absolutePositionActive = false;

void sendAbsoluteMouse() {
  const uint8_t report[] = {
    absoluteButtons,
    static_cast<uint8_t>(absoluteX), static_cast<uint8_t>(absoluteX >> 8),
    static_cast<uint8_t>(absoluteY), static_cast<uint8_t>(absoluteY >> 8)
  };
  HID().SendReport(kAbsoluteMouseReportId, report, sizeof(report));
}

uint8_t checksum(const uint8_t* bytes) {
  uint8_t value = 0;
  for (uint8_t i = 0; i < kPacketSize - 1; ++i) value ^= bytes[i];
  return value;
}

bool sendMouse(int8_t x, int8_t y, int8_t wheel = 0) {
  Mouse.move(x, y, wheel);
  return true;
}

void moveRelative(int16_t x, int16_t y) {
  const int32_t distance = max(abs(static_cast<int32_t>(x)), abs(static_cast<int32_t>(y)));
  const int32_t steps = (distance + 126) / 127;
  int32_t sentX = 0;
  int32_t sentY = 0;
  for (int32_t index = 1; index <= steps; ++index) {
    const int32_t nextX = static_cast<int32_t>(x) * index / steps;
    const int32_t nextY = static_cast<int32_t>(y) * index / steps;
    Mouse.move(nextX - sentX, nextY - sentY, 0);
    sentX = nextX;
    sentY = nextY;
  }
}

void reply(bool success) {
  Serial.write(kReply);
  Serial.write(success ? 0 : 1);
}

void execute() {
  if (packet[7] != checksum(packet)) {
    reply(false);
    return;
  }
  bool success = true;
  switch (packet[1]) {
    case 1: {
      const int16_t x = static_cast<int16_t>((packet[2] << 8) | packet[3]);
      const int16_t y = static_cast<int16_t>((packet[4] << 8) | packet[5]);
      moveRelative(x, y);
      break;
    }
    case 2: {
      const uint8_t mask = packet[2] == 1 ? 1 : packet[2] == 2 ? 2 : packet[2] == 3 ? 4 : 0;
      if (packet[3]) Mouse.press(mask); else Mouse.release(mask);
      if (absolutePositionActive && mask) {
        if (packet[3]) absoluteButtons |= mask; else absoluteButtons &= ~mask;
        sendAbsoluteMouse();
      }
      success = mask != 0;
      break;
    }
    case 3:
      success = sendMouse(0, 0, static_cast<int8_t>(packet[2]));
      break;
    case 4:
      success = sendMouse(100, 0);
      break;
    case 5: {
      absoluteX = static_cast<uint16_t>((packet[2] << 8) | packet[3]);
      absoluteY = static_cast<uint16_t>((packet[4] << 8) | packet[5]);
      absolutePositionActive = true;
      sendAbsoluteMouse();
      break;
    }
    case 6: {
      const uint8_t mask = packet[2] == 1 ? 1 : packet[2] == 2 ? 2 : packet[2] == 3 ? 4 : 0;
      if (!mask) {
        success = false;
        break;
      }
      Mouse.press(mask);
      if (absolutePositionActive) {
        absoluteButtons |= mask;
        sendAbsoluteMouse();
      }
      delay(2);
      Mouse.release(mask);
      if (absolutePositionActive) {
        absoluteButtons &= ~mask;
        sendAbsoluteMouse();
      }
      break;
    }
    default:
      success = false;
  }
  reply(success);
}
}

void setup() {
  Mouse.begin();
  Serial.begin(115200);
}

void loop() {
  while (Serial.available()) {
    const uint8_t value = Serial.read();
    if (packetLength == 0 && value != kHeader) continue;
    packet[packetLength++] = value;
    if (packetLength == kPacketSize) {
      execute();
      packetLength = 0;
    }
  }
}
