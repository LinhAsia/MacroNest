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
> Kết hợp bàn phím, chuột, OCR, tìm kiếm hình ảnh, phát hiện màu sắc, vẽ hình học, ghim một phần cửa sổ, lớp phủ hồng tâm, lệnh hệ thống, phát âm thanh, hiển thị HUD và nhiều tính năng khác trong cùng một luồng macro tự động với các biến số để xây dựng kịch bản linh hoạt.

## Tính năng chính

Các mô-đun dưới đây được thiết kế để hoạt động tương thích hoàn toàn với hệ thống macro, giúp bạn có thể kết hợp chúng trong cùng một luồng tự động hóa.

| Mô-đun | Chức năng | Cách hoạt động trong Macro |
| :--- | :--- | :--- |
| **Macro Engine** | Chạy phím bấm, hành động chuột, vòng lặp, chờ đợi và điều kiện rẽ nhánh | Xây dựng logic: click nút, điền thông tin, lặp lại công việc và rẽ nhánh luồng chạy bằng biến số |
| **Computer Vision** | Tìm ảnh trên màn hình, giám sát màu sắc và đếm số pixel trùng khớp | Quét màn hình: tự động tìm icon, đợi một vùng chuyển màu, hoặc kích hoạt khi thanh máu đầy |
| **OCR** | Trích xuất chữ/số từ màn hình vào biến số, kiểm tra sự tồn tại và tọa độ của văn bản | Đọc số liệu (tọa độ, điểm số) lưu vào biến và rẽ nhánh kịch bản khi văn bản khớp điều kiện |
| **Window Control** | Di chuyển, đổi kích thước, chia bố cục (layout), ghim nổi và phóng to cửa sổ | Thiết lập không gian: chia đôi màn hình ứng dụng, cắt bớt viền cửa sổ để click tọa độ chính xác |
| **Audio Sense** | Giám sát mức âm lượng và tần số (pitch) âm thanh hệ thống hoặc micrô | Nhận diện âm thanh: tự động phản hồi khi có cuộc gọi nói chuyện hoặc âm thanh game phát ra |
| **Sound Effects** | Phát âm thanh cảnh báo, đọc văn bản thành giọng nói (TTS) và clip tùy chỉnh | Cảnh báo bằng giọng nói: phát âm thanh khi có lỗi, hoặc thông báo trạng thái kịch bản đang chạy |
| **Crosshair** | Hiển thị hồng tâm tùy chỉnh màn hình theo phong cách riêng của bạn | Hỗ trợ ngắm bắn: hiển thị hồng tâm ảo trên màn hình, bật/tắt tự động bằng kịch bản kịch bản |
| **Geometry Overlay** | Vẽ điểm, đường thẳng, hình chữ nhật, hình tròn, elip, mũi tên, polyline, đa giác, cung tròn, nhãn chữ và SVG | Vẽ chỉ báo động: hiển thị vùng quét, mục tiêu di chuyển theo tọa độ biến số nhờ biểu thức toán học |
| **HUD Labels** | Hiển thị chữ viết và giá trị biến số nổi trên màn hình | Bảng theo dõi trực tiếp: hiển thị giá trị các biến số, bước kịch bản hiện tại đè lên màn hình |
| **Timer** | Tạo đồng hồ bấm giờ, đếm ngược và đếm thời gian hồi chiêu trên màn hình | Quản lý thời gian: đọc thời gian trôi qua/còn lại vào biến số (`timer1.second`, `timer1.raw`), kích hoạt hành động khi đếm xong |
| **Script Command** | Chạy trực tiếp lệnh CMD/PowerShell và lưu kết quả trả về vào biến số | Liên kết hệ thống: thực thi các công cụ dòng lệnh, chạy script cục bộ hoặc gọi API bên ngoài |
| **Hardware Input** | Giả lập chuột bàn phím qua Interception, Arduino, và đường chuột ghi lại | Mô phỏng nhập liệu: gửi tín hiệu chuột/bàn phím cấp thấp nhằm tương thích tối đa với các trò chơi hoặc ứng dụng bảo mật cao |

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
