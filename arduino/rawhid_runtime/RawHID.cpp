#include "RawHID.h"

static const uint8_t _hidReportDescriptorRawHID[] PROGMEM = {
    0x06, lowByte(RAWHID_USAGE_PAGE), highByte(RAWHID_USAGE_PAGE),
    0x0A, lowByte(RAWHID_USAGE), highByte(RAWHID_USAGE),
    0xA1, 0x01,
    0x75, 0x08,
    0x15, 0x00,
    0x26, 0xFF, 0x00,
    0x95, RAWHID_TX_SIZE,
    0x09, 0x01,
    0x81, 0x02,
    0x95, RAWHID_RX_SIZE,
    0x09, 0x02,
    0x91, 0x02,
    0xC0
};

RawHID_::RawHID_(void)
    : PluggableUSBModule(1, 1, epType),
      protocol(HID_REPORT_PROTOCOL),
      idle(1),
      dataLength(0),
      dataAvailable(0),
      data(nullptr) {
  epType[0] = EP_TYPE_INTERRUPT_IN;
  PluggableUSB().plug(this);
}

int RawHID_::getInterface(uint8_t* interfaceCount) {
  *interfaceCount += 1;
  HIDDescriptor hidInterface = {
      D_INTERFACE(pluggedInterface, 1, USB_DEVICE_CLASS_HUMAN_INTERFACE,
                  HID_SUBCLASS_NONE, HID_PROTOCOL_NONE),
      D_HIDREPORT(sizeof(_hidReportDescriptorRawHID)),
      D_ENDPOINT(USB_ENDPOINT_IN(pluggedEndpoint), USB_ENDPOINT_TYPE_INTERRUPT,
                 USB_EP_SIZE, 0x01)};
  return USB_SendControl(0, &hidInterface, sizeof(hidInterface));
}

int RawHID_::getDescriptor(USBSetup& setup) {
  if (setup.bmRequestType != REQUEST_DEVICETOHOST_STANDARD_INTERFACE) {
    return 0;
  }
  if (setup.wValueH != HID_REPORT_DESCRIPTOR_TYPE) {
    return 0;
  }
  if (setup.wIndex != pluggedInterface) {
    return 0;
  }

  protocol = HID_REPORT_PROTOCOL;
  return USB_SendControl(TRANSFER_PGM, _hidReportDescriptorRawHID,
                         sizeof(_hidReportDescriptorRawHID));
}

bool RawHID_::setup(USBSetup& setup) {
  if (pluggedInterface != setup.wIndex) {
    return false;
  }

  const uint8_t request = setup.bRequest;
  const uint8_t requestType = setup.bmRequestType;

  if (requestType == REQUEST_DEVICETOHOST_CLASS_INTERFACE) {
    if (request == HID_GET_REPORT || request == HID_GET_PROTOCOL) {
      return true;
    }
  }

  if (requestType == REQUEST_HOSTTODEVICE_CLASS_INTERFACE) {
    if (request == HID_SET_PROTOCOL) {
      protocol = setup.wValueL;
      return true;
    }
    if (request == HID_SET_IDLE) {
      idle = setup.wValueH;
      return true;
    }
    if (request == HID_SET_REPORT && setup.wValueH == HID_REPORT_TYPE_OUTPUT) {
      const int length = setup.wLength;
      if (!dataAvailable && length <= dataLength) {
        USB_RecvControl(data + dataLength - length, length);
        dataAvailable = length;
        return true;
      }
    }
  }

  return false;
}

RawHID_ RawHID;
