#include <Arduino.h>
#include <HID.h>
#include <PluggableUSB.h>

namespace {
const uint8_t mouseDescriptor[] PROGMEM = {
  0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x09, 0x01, 0xA1, 0x00,
  0x05, 0x09, 0x19, 0x01, 0x29, 0x08, 0x15, 0x00, 0x25, 0x01,
  0x95, 0x08, 0x75, 0x01, 0x81, 0x02,
  0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x38,
  0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x03, 0x81, 0x06,
  0xC0, 0xC0
};

class BootMouse : public PluggableUSBModule {
 public:
  BootMouse() : PluggableUSBModule(1, 1, endpointType), protocol(HID_REPORT_PROTOCOL), idle(1) {
    endpointType[0] = EP_TYPE_INTERRUPT_IN;
    PluggableUSB().plug(this);
  }

  bool send(uint8_t buttons, int8_t x, int8_t y, int8_t wheel) {
    const uint8_t report[4] = {buttons, static_cast<uint8_t>(x),
                               static_cast<uint8_t>(y), static_cast<uint8_t>(wheel)};
    const int length = protocol == HID_BOOT_PROTOCOL ? 3 : 4;
    return USB_Send(pluggedEndpoint | TRANSFER_RELEASE, report, length) == length;
  }

 protected:
  int getInterface(uint8_t* interfaceCount) override {
    *interfaceCount += 1;
    HIDDescriptor descriptor = {
      D_INTERFACE(pluggedInterface, 1, USB_DEVICE_CLASS_HUMAN_INTERFACE,
                  HID_SUBCLASS_BOOT_INTERFACE, HID_PROTOCOL_MOUSE),
      D_HIDREPORT(sizeof(mouseDescriptor)),
      D_ENDPOINT(USB_ENDPOINT_IN(pluggedEndpoint), USB_ENDPOINT_TYPE_INTERRUPT,
                 USB_EP_SIZE, 0x01)
    };
    return USB_SendControl(0, &descriptor, sizeof(descriptor));
  }

  int getDescriptor(USBSetup& setup) override {
    if (setup.wIndex != pluggedInterface ||
        setup.bmRequestType != REQUEST_DEVICETOHOST_STANDARD_INTERFACE) return 0;
    if (setup.wValueH == HID_HID_DESCRIPTOR_TYPE) {
      HIDDescDescriptor descriptor = D_HIDREPORT(sizeof(mouseDescriptor));
      return USB_SendControl(0, &descriptor, sizeof(descriptor));
    }
    if (setup.wValueH == HID_REPORT_DESCRIPTOR_TYPE) {
      protocol = HID_REPORT_PROTOCOL;
      return USB_SendControl(TRANSFER_PGM, mouseDescriptor, sizeof(mouseDescriptor));
    }
    return 0;
  }

  bool setup(USBSetup& setup) override {
    if (setup.wIndex != pluggedInterface) return false;
    if (setup.bmRequestType == REQUEST_HOSTTODEVICE_CLASS_INTERFACE) {
      if (setup.bRequest == HID_SET_PROTOCOL) {
        protocol = setup.wValueL;
        return true;
      }
      if (setup.bRequest == HID_SET_IDLE) {
        idle = setup.wValueH;
        return true;
      }
    }
    return setup.bmRequestType == REQUEST_DEVICETOHOST_CLASS_INTERFACE &&
           (setup.bRequest == HID_GET_REPORT || setup.bRequest == HID_GET_PROTOCOL ||
            setup.bRequest == HID_GET_IDLE);
  }

 private:
  uint8_t endpointType[1];
  uint8_t protocol;
  uint8_t idle;
};

BootMouse mouse;
constexpr uint8_t packetSize = 6;
uint8_t packet[packetSize];
uint8_t packetLength = 0;
uint8_t buttons = 0;

void handlePacket() {
  bool sent = true;
  switch (packet[1]) {
    case 1: {
      const int16_t x = static_cast<int16_t>((packet[2] << 8) | packet[3]);
      const int16_t y = static_cast<int16_t>((packet[4] << 8) | packet[5]);
      mouse.send(buttons, static_cast<int8_t>(x), static_cast<int8_t>(y), 0);
      break;
    }
    case 2: {
      const uint8_t button = packet[2] == 1 ? 1 : packet[2] == 2 ? 2 : packet[2] == 3 ? 4 : 0;
      if (packet[3]) buttons |= button; else buttons &= ~button;
      mouse.send(buttons, 0, 0, 0);
      break;
    }
    case 3:
      mouse.send(buttons, 0, 0, static_cast<int8_t>(packet[2]));
      break;
    case 0x7F:
      for (uint8_t i = 0; i < 5; ++i) {
        sent &= mouse.send(0, 20, 0, 0);
        delay(20);
      }
      sent &= mouse.send(1, 0, 0, 0);
      delay(20);
      sent &= mouse.send(0, 0, 0, 0);
      Serial.write(sent ? 0xAC : 0xE1);
      break;
  }
}
}

void setup() {
  Serial.begin(115200);
  delay(3000);
  for (uint8_t i = 0; i < 10; ++i) {
    mouse.send(0, 20, 0, 0);
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
