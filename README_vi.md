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
| **Timer** | Tạo đồng hồ bấm giờ, đếm ngược và đếm thời gian hồi chiêu trên màn hình | Quản lý thời gian: đọc giá trị đồng hồ bấm giờ/đếm ngược vào biến số thông qua hành động macro, kích hoạt hành động khi đếm xong, hoặc hiển thị thời gian hồi chiêu của chiêu thức |
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

## Trợ giúp Công thức

<details>
  <summary>Xem chi tiết cú pháp biểu thức và các ví dụ</summary>

### Toán tử

| Cú pháp | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `a + b` | Cộng | `2 + 3` | `5` |
| `a - b` | Trừ | `10 - 4` | `6` |
| `a * b` | Nhân | `3 * 4` | `12` |
| `a / b` | Chia | `5 / 2` | `2.5` |
| `a ^ b` | Lũy thừa | `5^2` | `25` |

### Hằng số

| Cú pháp | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `pi` | Hằng số Pi | `degrees(pi)` | `180` |
| `e` | Số Euler | `round(e, 3)` | `2.718` |

### Hàm Cơ bản

| Hàm | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `random(min, max)` | Số nguyên ngẫu nhiên trong khoảng | `random(10, 20)` | `10..20` |
| `choice(a, b, ...)` | Chọn ngẫu nhiên một giá trị (hỗ trợ số, chữ hoặc kết hợp) | 1. `choice(10, 20, 30)` (số)<br>2. `choice(apple, banana, cherry)` (chữ)<br>3. `choice(HP: 100, 20, low)` (kết hợp) | 1. `10` hoặc `20` hoặc `30`<br>2. `apple` hoặc `banana` hoặc `cherry`<br>3. `HP: 100` hoặc `20` hoặc `low` |
| `min(a, b)` | Giá trị nhỏ hơn | `min(20, 50)` | `20` |
| `max(a, b)` | Giá trị lớn hơn | `max(20, 50)` | `50` |
| `abs(a)` | Giá trị tuyệt đối | `abs(-50)` | `50` |
| `div(a, b)` | Chia lấy phần nguyên (làm tròn xuống) | `div(5, 2)` | `2` |
| `mod(a, b)` | Chia lấy phần dư | `mod(5, 2)` | `1` |
| `round(a, digits)` | Làm tròn tới số chữ số thập phân | `round(863.6897, 2)` | `863.69` |
| `ceil(a)` | Làm tròn lên | `ceil(pi)` | `4` |
| `floor(a)` | Làm tròn xuống | `floor(pi)` | `3` |
| `sqrt(a)` | Căn bậc hai | `sqrt(9)` | `3` |
| `pow(a, b)` | Hàm lũy thừa | `pow(2, 3)` | `8` |
| `factorial(n)` | Giai thừa | `factorial(5)` | `120` |
| `gcd(a, b, ...)` | Ước chung lớn nhất | `gcd(24, 36, 48)` | `12` |
| `lcm(a, b, ...)` | Bội chung nhỏ nhất | `lcm(4, 6, 8)` | `24` |
| `isqrt(n)` | Căn bậc hai lấy phần nguyên | `isqrt(17)` | `4` |
| `comb(n, k)` | Tổ hợp chập k của n | `comb(5, 2)` | `10` |
| `perm(n, k)` | Chỉnh hợp chập k của n | `perm(5, 2)` | `20` |

### Lượng giác và Góc

| Hàm | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `sin(a)` | Sin | `sin(radians(30)) * 1000` | `500` |
| `cos(a)` | Cos | `cos(radians(60)) * 1000` | `500` |
| `tan(a)` | Tan | `tan(45)` | phụ thuộc đơn vị đầu vào |
| `asin(a)` | Arc sin | `asin(0.5)` | góc tính bằng radian |
| `acos(a)` | Arc cos | `acos(0.5)` | góc tính bằng radian |
| `atan(a)` | Arc tan | `degrees(atan(1))` | `45` |
| `atan2(y, x)` | Arc tan 2 tham số | `degrees(atan2(1, 1))` | `45` |
| `sinh(a)` | Sin hyperbolic | `sinh(1)` | kết quả số |
| `cosh(a)` | Cos hyperbolic | `cosh(1)` | kết quả số |
| `tanh(a)` | Tan hyperbolic | `tanh(1)` | kết quả số |
| `degrees(rad)` | Radian sang độ | `degrees(pi)` | `180` |
| `radians(deg)` | Độ sang radian | `radians(180)` | khoảng `3.14159` |

### Logarit và Mũ

| Hàm | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `ln(a)` | Logarit tự nhiên | `ln(e)` | `1` |
| `log(a)` | Logarit tự nhiên | `log(e)` | `1` |
| `log10(a)` | Logarit cơ số 10 | `log10(1000)` | `3` |
| `exp(a)` | Hàm mũ `e^a` | `exp(1)` | khoảng `2.71828` |

### Trợ giúp Văn bản

| Hàm | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `contains(a, b)` | Kiểm tra chuỗi `a` có chứa chuỗi `b` hay không (hỗ trợ số, chữ hoặc kết hợp) | 1. `contains(hello, el)` (chữ)<br>2. `contains(HP: 100, 100)` (kết hợp)<br>3. `contains(12345, 34)` (số) | 1. `1` (đúng)<br>2. `1` (đúng)<br>3. `1` (đúng) |
| `substr(text, start, len)` | Lấy một chuỗi con (hỗ trợ số, chữ hoặc kết hợp) | 1. `substr(banana, 2, 3)` (chữ)<br>2. `substr(HP: 100, 4, 3)` (kết hợp)<br>3. `substr(123456, 1, 4)` (số) | 1. `nan`<br>2. `100`<br>3. `2345` |
| `len(text)` | Đếm số ký tự (hỗ trợ số, chữ hoặc kết hợp) | 1. `len(apple)` (chữ)<br>2. `len(HP: 100)` (kết hợp)<br>3. `len(453454)` (số) | 1. `5`<br>2. `7`<br>3. `6` |
| `myVar.toNumber` | Trích xuất các chữ số từ biến văn bản và chuyển thành số (bỏ qua ký tự khác) | Nếu biến `A` là `"HP: 120"` (văn bản):<br>`A.toNumber + 5` | `125` (dạng số) |
| `myVar.toString` | Lọc bỏ toàn bộ chữ số và giữ lại các ký tự khác để chuyển thành văn bản | 1. Nếu `A` là `123` (số): `A.toString`<br>2. Nếu `A` là `"123abc"` (văn bản): `A.toString` | 1. `"123"` (văn bản)<br>2. `"abc"` (văn bản) |

### Biến Hệ thống Có sẵn (Dạng Số)

| Biến số | Ý nghĩa | Ví dụ / Ghi chú |
| :--- | :--- | :--- |
| `screen.width` | Chiều rộng của màn hình chính (pixel) | `screen.width` |
| `screen.height` | Chiều cao của màn hình chính (pixel) | `screen.height` |
| `mouse.x` | Tọa độ X hiện tại của con trỏ chuột | `mouse.x` |
| `mouse.y` | Tọa độ Y hiện tại của con trỏ chuột | `mouse.y` |
| `mouse.sensitivity` | Tốc độ nhạy của chuột hệ thống | `mouse.sensitivity` |
| `volume.level` | Mức âm lượng hệ thống hiện tại (0 đến 100) | `volume.level` |
| `window.x` hoặc `left` | Tọa độ X cạnh trái của cửa sổ mục tiêu | `window.x` |
| `window.y` hoặc `top` | Tọa độ Y cạnh trên của cửa sổ mục tiêu | `window.y` |
| `window.right` | Tọa độ X cạnh phải của cửa sổ mục tiêu | `window.right` |
| `window.bottom` | Tọa độ Y cạnh dưới của cửa sổ mục tiêu | `window.bottom` |
| `window.width` | Chiều rộng của cửa sổ mục tiêu | `window.width` |
| `window.height` | Chiều cao của cửa sổ mục tiêu | `window.height` |
| `window.centerX` | Tọa độ X tâm của cửa sổ mục tiêu | `window.centerX` |
| `window.centerY` | Tọa độ Y tâm của cửa sổ mục tiêu | `window.centerY` |

### Biến Hệ thống Có sẵn (Thời gian và Chữ)

| Biến / Thuộc tính | Ý nghĩa | Ví dụ / Ghi chú |
| :--- | :--- | :--- |
| `system.year` / `month` / `day` | Năm, tháng, hoặc ngày hiện tại của hệ thống | `system.year` |
| `system.hour` / `minute` / `second` | Giờ, phút, hoặc giây hiện tại của hệ thống | `system.hour` |
| `system.millisecond` | Mili-giây hiện tại của hệ thống | `system.millisecond` |
| `system.date` | Ngày hệ thống hiện tại dưới dạng chuỗi | ví dụ `2026-07-09` |
| `system.time` | Giờ hệ thống hiện tại dưới dạng chuỗi | ví dụ `04:24:00` |
| `window.title` | Tiêu đề của cửa sổ mục tiêu | `window.title` |
| `clipboard.text` | Nội dung văn bản hiện tại trong clipboard | `clipboard.text` |
| `timer1.hour` / `minute` / `second` / `total_sec` | Giá trị của bộ đếm thời gian tích hợp sẵn | ví dụ `timer1.total_sec`. Thay `timer1` bằng tên timer tùy chỉnh của bạn nếu có |

### Ghi chú

- Các trường biểu thức tính toán trực tiếp các biến và hàm số.
- Các trường văn bản thuần sẽ giữ nguyên chữ thường; dùng `{...}` để truyền biến hoặc phép toán vào văn bản.
- Một số trường macro lưu giá trị cuối cùng dạng số nguyên, vì vậy kết quả thập phân có thể bị làm tròn tại đó.

</details>

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
