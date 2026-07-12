#include <Arduino.h>
#include <Mouse.h>

namespace {
constexpr uint8_t kPacketSize = 8;
constexpr uint8_t kHeader = 0xA5;
constexpr uint8_t kReply = 0x5A;
uint8_t packet[kPacketSize];
uint8_t packetLength = 0;

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
      success = mask != 0;
      break;
    }
    case 3:
      success = sendMouse(0, 0, static_cast<int8_t>(packet[2]));
      break;
    case 4:
      success = sendMouse(100, 0);
      break;
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
