#include <Arduino.h>
#include <HID.h>

namespace {
constexpr uint8_t kPacketSize = 8;
constexpr uint8_t kHeader = 0xA5;
constexpr uint8_t kReply = 0x5A;
uint8_t packet[kPacketSize];
uint8_t packetLength = 0;
uint8_t buttons = 0;

const uint8_t mouseDescriptor[] PROGMEM = {
  0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x09, 0x01, 0xA1, 0x00,
  0x85, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x05, 0x15, 0x00,
  0x25, 0x01, 0x95, 0x05, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01,
  0x75, 0x03, 0x81, 0x03, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31,
  0x09, 0x38, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x03,
  0x81, 0x06, 0xC0, 0xC0
};

HIDSubDescriptor descriptor(mouseDescriptor, sizeof(mouseDescriptor));
struct RegisterMouseDescriptor {
  RegisterMouseDescriptor() { HID().AppendDescriptor(&descriptor); }
} registerMouseDescriptor;

uint8_t checksum(const uint8_t* bytes) {
  uint8_t value = 0;
  for (uint8_t i = 0; i < kPacketSize - 1; ++i) value ^= bytes[i];
  return value;
}

bool sendMouse(int8_t x, int8_t y, int8_t wheel = 0) {
  const uint8_t report[4] = {buttons, static_cast<uint8_t>(x),
                             static_cast<uint8_t>(y), static_cast<uint8_t>(wheel)};
  return HID().SendReport(1, report, sizeof(report)) == 5;
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
    case 0:
      break;
    case 1: {
      const int16_t x = static_cast<int16_t>((packet[2] << 8) | packet[3]);
      const int16_t y = static_cast<int16_t>((packet[4] << 8) | packet[5]);
      success = sendMouse(static_cast<int8_t>(x), static_cast<int8_t>(y));
      break;
    }
    case 2: {
      const uint8_t mask = packet[2] == 1 ? 1 : packet[2] == 2 ? 2 : packet[2] == 3 ? 4 : 0;
      if (packet[3]) buttons |= mask; else buttons &= ~mask;
      success = mask != 0 && sendMouse(0, 0);
      break;
    }
    case 3:
      success = sendMouse(0, 0, static_cast<int8_t>(packet[2]));
      break;
    case 4:
      for (uint8_t i = 0; i < 5; ++i) success &= sendMouse(20, 0);
      break;
    default:
      success = false;
  }
  reply(success);
}
}

void setup() {
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
