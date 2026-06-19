<p align="left">
  <img src="assets/banner-v4.svg" alt="MacroNest Banner" width="100%" />
  <a href="https://github.com/NBaoLinh/MacroNest/stargazers"><img src="assets/star-button-v2.svg" alt="Star MacroNest" height="38" /></a>
  <a href="https://github.com/NBaoLinh/MacroNest/releases/latest"><img src="assets/download-button-v2.svg" alt="Download MacroNest" height="38" /></a>
  <a href="README.md"><img src="assets/lang-en-button-v2.svg" alt="English" height="38" /></a>
</p>

## 🌟 Tính năng chính

| Phân hệ | Mô tả chi tiết | Tích hợp Macro |
| :--- | :--- | :--- |
| **⌨️ Macro Engine** | • Tự động hóa chuỗi phím, chuột, vòng lặp và câu lệnh điều kiện<br>• Hệ thống biến tập trung hỗ trợ tính toán biểu thức toán học | *Bộ điều phối trung tâm cho mọi hành động tự động.* |
| **👁️ Computer Vision** | • Phát hiện hình ảnh trên màn hình (OpenCV Template Matching)<br>• Theo dõi thay đổi pixel & màu sắc trong vùng chỉ định<br>• Đếm số lượng pixel khớp với màu mục tiêu trong vùng | *Kích hoạt phím hoặc chuột khi hình ảnh/màu sắc xuất hiện.* |
| **📝 Nhận diện chữ (OCR)** | • Windows Native OCR độ trễ cực thấp để nhận diện văn bản<br>• Khớp chữ viết, mẫu ký tự hoặc số trên màn hình | *Chạy macro ngay lập tức khi phát hiện từ hoặc số cụ thể.* |
| **🪟 Điều khiển cửa sổ** | • Thay đổi kích thước/vị trí cửa sổ theo neo hoặc **Snap Layouts** (lưới bố cục)<br>• **Live DWM Pinning**: Ghim một vùng cắt của cửa sổ bất kỳ luôn trên cùng<br>• Hỗ trợ chọn cửa sổ, khóa tỷ lệ khung hình và thu phóng | *Tự động focus hoặc sắp xếp cửa sổ theo lưới thời gian thực.* |
| **🎙️ Cảm biến âm thanh** | • Theo dõi mức âm lượng và tần số từ hệ thống hoặc micro | *Kích hoạt macro ngay khi đạt ngưỡng âm lượng/tần số thiết lập.* |
| **🎵 Hiệu ứng âm thanh** | • Kích hoạt âm báo tùy chỉnh và cắt các đoạn âm thanh | *Phát âm thanh báo hiệu khi macro chạy xong hoặc gặp lỗi.* |
| **➕ Tâm ngắm (Crosshair)** | • Hiển thị tâm ngắm (chấm, chữ thập, vòng tròn) tùy biến màu và độ mờ | *Bật/tắt hoặc đổi tâm ngắm dựa trên trạng thái macro.* |
| **📐 Vẽ hình học (Geometry)** | • Vẽ đường thẳng, hộp thoại, vòng tròn và hình học động lên màn hình | *Đánh dấu mục tiêu hoặc vẽ khung nhận diện động trên màn hình.* |
| **🏷️ Nhãn HUD hiển thị** | • Hiển thị văn bản, bộ đếm thời gian và đếm ngược trên màn hình overlay | *Hiển thị giá trị của biến hoặc các bước thực thi macro.* |
| **📜 Chạy tập lệnh** | • Thực thi trực tiếp các tập lệnh CMD và PowerShell | *Chạy các lệnh hệ thống tích hợp trong chuỗi hành động macro.* |
| **🖱️ Mô phỏng phần cứng** | • **Interception & Arduino**: Mô phỏng phím/chuột cấp trình điều khiển (driver) và phần cứng giúp vượt anti-cheat<br>• **Mouse Path**: Ghi và phát lại quỹ đạo di chuyển chuột mượt mà | *Phát lại đường chuột hoặc đổi độ nhạy DPI tức thời.* |
| **⚡ Quick Actions (Nhanh)** | • Bật/tắt nhanh thanh tác vụ, khóa phím Windows, ghim cửa sổ luôn trên cùng<br>• Viền nổi bật cửa sổ active, công cụ vẽ màn hình, thước đo góc và linh vật gõ phím | *Các tiện ích hệ thống nhanh truy cập trực tiếp từ thanh tiêu đề.* |

---

## 🚀 Bắt đầu sử dụng

### Yêu cầu hệ thống

| Tiêu chuẩn | Tối thiểu |
| :--- | :--- |
| **Hệ điều hành** | Windows 10 / 11 (64-bit) |
| **Runtime** | Không cần cài đặt (phiên bản `.exe` di động) |
| **Quyền hạn** | Administrator (bắt buộc chạy dưới quyền quản trị để truyền đầu vào và quản lý cửa sổ toàn hệ thống) |

### Cài đặt

1. Tải về **`MacroNest.exe`** từ trang [Bản phát hành mới nhất](https://github.com/NBaoLinh/MacroNest/releases/latest).
2. Khởi chạy trực tiếp ứng dụng. Không cần cài đặt.

### Thư viện bổ sung (Tải trực tiếp trong Cài đặt)

- **OpenCV DLL**: Cần thiết cho tính năng Computer Vision (Tìm kiếm hình ảnh).
- **Interception Driver**: Cần thiết cho mô phỏng phím/chuột cấp driver.
- **Arduino Firmware**: Cần thiết cho mô phỏng phím/chuột cấp phần cứng qua mạch Arduino.
