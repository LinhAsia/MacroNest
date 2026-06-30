<p align="left">
  <a href="https://github.com/LinhAsia/MacroNest">
    <img src="assets/banner-v4.svg" alt="MacroNest Banner" width="100%" />
  </a>
  <a href="https://github.com/LinhAsia/MacroNest/stargazers"><img src="assets/star-button-v2.svg" alt="Star MacroNest" height="38" /></a>
  <a href="https://github.com/LinhAsia/MacroNest/releases/latest"><img src="assets/download-button-v2.svg" alt="Tải về MacroNest" height="38" /></a>
  <a href="README.md"><img src="assets/lang-en-button-v2.svg" alt="English" height="38" /></a>
</p>

> **MacroNest là một công cụ tự động hóa và macro trên nền tảng Windows hoàn toàn miễn phí và mã nguồn mở.**
>
> Được xây dựng chú trọng vào bảo mật, sự minh bạch và hiệu năng cao, MacroNest cho phép bạn kết hợp các phím nhấn bàn phím, hành động chuột, OCR (nhận diện chữ viết), tìm kiếm hình ảnh (image search), phát hiện màu sắc (color detect), vẽ hình học (geometry drawing), ghim/phóng to một phần của cửa sổ (window pin/zoom), lớp phủ hồng tâm (crosshair overlay), lệnh hệ thống (script command), phát âm thanh, hiển thị HUD thông tin và nhiều tính năng khác trong cùng một luồng macro tự động linh hoạt có sử dụng biến số.
>
> > [!IMPORTANT]
> > **Lưu ý về Bảo mật & Độ tin cậy**
> > Vì MacroNest quản lý các hook bàn phím/chuột cấp thấp và chụp các vùng màn hình để tìm kiếm hình ảnh/OCR, nên phần mềm được xây dựng **mã nguồn mở 100%** để đảm bảo tính minh bạch hoàn toàn. Ứng dụng chạy hoàn toàn cục bộ trên máy tính của bạn với **không có telemetry (thu thập dữ liệu ẩn), không thu thập thông tin và không kết nối Internet** (trừ khi bạn chủ động tải xuống các mô hình OCR tùy chọn hoặc kiểm tra phiên bản mới). Bạn có thể kiểm tra từng dòng mã nguồn để xác thực độ an toàn của nó.

## Tính năng chính

Các mô-đun dưới đây được thiết kế để hoạt động tương thích hoàn toàn với hệ thống macro, giúp bạn có thể kết hợp chúng trong cùng một luồng tự động hóa.

| Mô-đun | Chức năng | Cách hoạt động trong Macro |
| :--- | :--- | :--- |
| Macro Engine | Chạy phím bấm, hành động chuột, vòng lặp, chờ đợi và điều kiện rẽ nhánh | Xây dựng luồng chính cho kịch bản tự động hóa |
| Computer Vision | Tìm ảnh trên màn hình, giám sát màu sắc và đếm số pixel trùng khớp | Kích hoạt hành động khi một hình ảnh/màu sắc xuất hiện trên màn hình |
| OCR | Đọc chữ và số trên màn hình cực nhanh bằng PaddleOCR cục bộ | Chạy macro khi phát hiện nội dung văn bản trùng khớp |
| Window Control | Di chuyển, đổi kích thước, ghim nổi và thu phóng các cửa sổ ứng dụng | Kiểm soát không gian làm việc của bạn trong khi macro đang chạy |
| Audio Sense | Giám sát mức âm lượng và tần số âm thanh hệ thống hoặc micrô | Kích hoạt hành động tự động từ tín hiệu âm thanh |
| Sound Effects | Phát âm thanh cảnh báo và sử dụng các clip âm thanh tùy chỉnh | Xác nhận trạng thái, cảnh báo hoặc các sự kiện macro |
| Crosshair | Hiển thị hồng tâm tùy chỉnh màn hình theo phong cách riêng của bạn | Bật hoặc tắt lớp phủ hồng tâm bằng phím tắt hoặc macro |
| Geometry Overlay | Vẽ đường thẳng, hộp thoại, hình tròn và các hình dạng hình học khác | Đánh dấu các mục tiêu trên màn hình trực quan trong khi chạy macro |
| HUD Labels | Hiển thị chữ viết, đồng hồ đếm giờ và bộ đếm ngược trên màn hình | Hiển thị các giá trị và tiến trình của macro khi đang chạy |
| Script Command | Chạy trực tiếp lệnh CMD và PowerShell | Gọi các đoạn mã kịch bản hệ thống ngay bên trong luồng macro |
| Hardware Input | Hỗ trợ mô phỏng chuột bàn phím qua Interception, Arduino, và đường di chuyển chuột ghi lại | Gửi tín hiệu nhập liệu ở cấp độ phần cứng khi cần |

## Thao tác Nhanh (Quick Actions)

Quick Actions là các công cụ tiện ích nhỏ nằm trên thanh tiêu đề để bạn truy cập thủ công nhanh chóng, hoạt động độc lập với luồng macro ở trên.

| Thao tác | Mô tả chức năng |
| :--- | :--- |
| Taskbar | Ẩn hoặc hiển thị lại thanh Taskbar của Windows |
| Windows Key | Khóa hoặc mở khóa phím Windows trên bàn phím |
| Window Pin | Ghim nổi một cửa sổ ứng dụng bất kỳ luôn nằm trên cùng |
| Focus Highlight | Làm nổi bật cửa sổ đang hoạt động bằng đường viền và hiệu ứng có thể cấu hình |
| Protractor | Hiển thị thước đo góc kéo thả trực quan trên màn hình để kiểm tra góc |
| Ruler | Đo khoảng cách giữa hai điểm trên màn hình và tùy chọn sao chép kết quả |
| Get Coordinates | Lấy tọa độ một điểm trên màn hình và tùy chọn sao chép giá trị X, Y |
| Get Color | Lấy mẫu màu màn hình và tùy chọn sao chép mã màu Hex |
| Key Display | Hiển thị phím nhấn thời gian thực với chế độ Normal và Mascot hoạt hình dễ thương |
| Draw | Bật/tắt lớp phủ vẽ tự do trên màn hình và định hình phím tắt cho nó |
| Clear Overlays | Xóa nhanh tất cả hình vẽ hình học, HUD và các lớp ghim nổi đang hiển thị |
| Key Sound | Phát âm thanh gõ phím giả lập cơ học với nhiều loại switch và âm lượng |

## Bắt đầu Sử dụng

### Yêu cầu Hệ thống

| Yêu cầu | Tối thiểu |
| :--- | :--- |
| Hệ điều hành | Windows 10 / 11 (64-bit) |
| Runtime | Không cần cài đặt, phiên bản Portable `.exe` chạy ngay |
| Quyền hạn | Quyền Quản trị viên (Administrator access) |

### Cài đặt

1. Tải tệp **`MacroNest.exe`** từ [phiên bản mới nhất](https://github.com/LinhAsia/MacroNest/releases/latest).
2. Chạy tệp tin để sử dụng trực tiếp.

### Tùy chọn Tải thêm

Bạn có thể tải các thành phần này trực tiếp từ phần cài đặt của ứng dụng:

- Thư viện OpenCV DLL để phục vụ cho tính năng tìm kiếm hình ảnh
- Trình điều khiển Interception để mô phỏng chuột/bàn phím cấp thấp
- Firmware Arduino để giả lập nhập liệu phần cứng thông qua mạch ngoại vi

## Bản quyền

Phát hành dưới giấy phép MIT License. Xem chi tiết tại [LICENSE](LICENSE).
