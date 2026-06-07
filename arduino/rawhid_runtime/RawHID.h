#pragma once

#include <Arduino.h>
#include "HID.h"
#include "PluggableUSB.h"

#define RAWHID_USAGE_PAGE 0xFFC0
#define RAWHID_USAGE 0x0C00
#define RAWHID_TX_SIZE USB_EP_SIZE
#define RAWHID_RX_SIZE USB_EP_SIZE

#ifndef HID_REPORT_TYPE_INPUT
#define HID_REPORT_TYPE_INPUT 1
#endif

#ifndef HID_REPORT_TYPE_OUTPUT
#define HID_REPORT_TYPE_OUTPUT 2
#endif

#ifndef HID_REPORT_TYPE_FEATURE
#define HID_REPORT_TYPE_FEATURE 3
#endif

#ifndef ATTRIBUTE_PACKED
#define ATTRIBUTE_PACKED
#endif

typedef union ATTRIBUTE_PACKED {
  uint8_t buff[RAWHID_TX_SIZE];
} HID_RawKeyboardTXReport_Data_t;

typedef union ATTRIBUTE_PACKED {
  uint8_t buff[RAWHID_RX_SIZE];
} HID_RawKeyboardRXReport_Data_t;

class RawHID_ : public PluggableUSBModule, public Stream {
 public:
  RawHID_(void);

  void begin(void* report, int length) {
    if (length > 0) {
      data = static_cast<uint8_t*>(report);
      dataLength = length;
      dataAvailable = 0;
    }
  }

  void end(void) {
    disable();
    dataLength = 0;
  }

  void enable(void) { dataAvailable = 0; }
  void disable(void) { dataAvailable = -1; }

  virtual int available(void) {
    if (dataAvailable < 0) {
      return 0;
    }
    return dataAvailable;
  }

  virtual int read() {
    if (dataAvailable > 0) {
      return data[dataLength - dataAvailable--];
    }
    return -1;
  }

  virtual int peek() {
    if (dataAvailable > 0) {
      return data[dataLength - dataAvailable];
    }
    return -1;
  }

  virtual void flush(void) {}

  using Print::write;
  virtual size_t write(uint8_t b) { return write(&b, 1); }
  virtual size_t write(uint8_t* buffer, size_t size) {
    return USB_Send(pluggedEndpoint | TRANSFER_RELEASE, buffer, size);
  }

 protected:
  int getInterface(uint8_t* interfaceCount) override;
  int getDescriptor(USBSetup& setup) override;
  bool setup(USBSetup& setup) override;

  uint8_t epType[1];
  uint8_t protocol;
  uint8_t idle;
  int dataLength;
  int dataAvailable;
  uint8_t* data;
};

extern RawHID_ RawHID;
