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
| **Macro Engine** | Chạy phím bấm, hành động chuột, vòng lặp, chờ đợi, điều kiện rẽ nhánh và chia sẻ kịch bản | Xây dựng logic: click nút, điền thông tin, lặp lại công việc, rẽ nhánh bằng biến số, và xuất/nhập mã chia sẻ kịch bản nén qua clipboard |
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
| Focus Mode | Làm tối toàn bộ không gian bên ngoài cửa sổ đang focus hoặc được chọn |
| Window Opacity | Điều chỉnh độ trong suốt gốc của cửa sổ được chọn trực tiếp từ 0% đến 100% |
| Protractor | Hiển thị thước đo góc kéo thả trực quan trên màn hình để kiểm tra góc |
| Ruler | Đo khoảng cách giữa hai điểm trên màn hình và tùy chọn sao chép kết quả |
| Get Coordinates | Lấy tọa độ một điểm trên màn hình và tùy chọn sao chép giá trị X, Y |
| Get Color | Lấy mẫu màu màn hình và tùy chọn sao chép mã màu Hex |
| Key Display | Hiển thị phím nhấn thời gian thực với chế độ Normal và Mascot hoạt hình dễ thương |
| Draw | Bật/tắt lớp phủ vẽ tự do trên màn hình và định hình phím tắt cho nó |
| Quay màn hình | Quay toàn màn hình, cửa sổ đang focus, cửa sổ đã chọn hoặc một vùng đã chọn ở 30, 60 hoặc 144 FPS kèm âm thanh WASAPI hệ thống. Tích hợp Thư viện Video (Video Library) với trình phát 60 FPS mượt mà, công cụ cắt (Trim) và nén (Compress) video trực tiếp trong app |
| Clear Overlays | Xóa nhanh tất cả hình vẽ hình học, HUD và các lớp ghim nổi đang hiển thị |
| Key Sound | Phát âm thanh gõ phím giả lập cơ học với nhiều loại switch và âm lượng |

## Trợ giúp Công thức

<details>
  <summary>Xem chi tiết cú pháp biểu thức và các ví dụ</summary>

### Toán tử

| Cú pháp | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `a + b` | Phép cộng | `2 + 3` | `5` |
| `a - b` | Phép trừ | `10 - 4` | `6` |
| `a * b` | Phép nhân | `3 * 4` | `12` |
| `a / b` | Phép chia | `5 / 2` | `2.5` |
| `a ^ b` | Lũy thừa | `5^2` | `25` |
| `a == b` | So sánh bằng | `5 == 5` | `1` |
| `a != b` | So sánh khác | `5 != 5` | `0` |
| `a > b` / `a >= b` | Lớn hơn / Lớn hơn hoặc bằng | `8 >= 3` | `1` |
| `a < b` / `a <= b` | Nhỏ hơn / Nhỏ hơn hoặc bằng | `2 < 1` | `0` |

### Hằng số

| Cú pháp | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `pi` | Hằng số Pi | `degrees(pi)` | `180` |
| `e` | Số Euler | `round(e, 3)` | `2.718` |

### Hàm Cơ bản

| Hàm | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `random(min, max)` | Số nguyên ngẫu nhiên trong khoảng | `random(10, 20)` | `10..20` |
| `choice(a, b, ...)` | Chọn ngẫu nhiên một giá trị (hỗ trợ số, chữ hoặc kết hợp) | 1. `choice(10, 20, 30)` (số)<br>2. `choice(táo, chuối, cam)` (chữ)<br>3. `choice(Cấp: 5, 50, chí mạng)` (kết hợp) | 1. `10` hoặc `20` hoặc `30`<br>2. `táo` hoặc `chuối` hoặc `cam`<br>3. `Cấp: 5` hoặc `50` hoặc `chí mạng` |
| `clamp(x, min, max)` | Giới hạn `x` trong khoảng | `clamp(120, 0, 100)` | `100` |
| `between(x, a, b)` | Kiểm tra `x` có nằm trong khoảng không (bao gồm hai đầu) | `between(7, 1, 10)` | `1` |
| `min(a, b)` | Giá trị nhỏ hơn | `min(20, 50)` | `20` |
| `max(a, b)` | Giá trị lớn hơn | `max(20, 50)` | `50` |
| `abs(a)` | Giá trị tuyệt đối | `abs(-50)` | `50` |
| `div(a, b)` | Phép chia lấy phần nguyên | `div(5, 2)` | `2` |
| `mod(a, b)` | Phép chia lấy phần dư | `mod(5, 2)` | `1` |
| `round(a, digits)` | Làm tròn số thập phân | `round(863.6897, 2)` | `863.69` |
| `ceil(a)` | Làm tròn lên | `ceil(pi)` | `4` |
| `floor(a)` | Làm tròn xuống | `floor(pi)` | `3` |
| `sqrt(a)` | Căn bậc hai | `sqrt(9)` | `3` |
| `pow(a, b)` | Hàm lũy thừa | `pow(2, 3)` | `8` |
| `factorial(n)` | Giai thừa | `factorial(5)` | `120` |
| `gcd(a, b, ...)` | Ước chung lớn nhất | `gcd(24, 36, 48)` | `12` |
| `lcm(a, b, ...)` | Bội chung nhỏ nhất | `lcm(4, 6, 8)` | `24` |
| `isqrt(n)` | Căn bậc hai số nguyên | `isqrt(17)` | `4` |
| `comb(n, k)` | Tổ hợp | `comb(5, 2)` | `10` |
| `perm(n, k)` | Chỉnh hợp | `perm(5, 2)` | `20` |

### Lượng giác và Góc

| Hàm | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `sin(a)` | Sin | `sin(radians(30)) * 1000` | `500` |
| `cos(a)` | Cos | `cos(radians(60)) * 1000` | `500` |
| `tan(a)` | Tan | `tan(45)` | phụ thuộc đơn vị đầu vào |
| `asin(a)` | Arc sin | `asin(0.5)` | góc theo radian |
| `acos(a)` | Arc cos | `acos(0.5)` | góc theo radian |
| `atan(a)` | Arc tan | `degrees(atan(1))` | `45` |
| `atan2(y, x)` | Arc tan 2 đối số | `degrees(atan2(1, 1))` | `45` |
| `sinh(a)` | Hyperbolic sin | `sinh(1)` | kết quả số |
| `cosh(a)` | Hyperbolic cos | `cosh(1)` | kết quả số |
| `tanh(a)` | Hyperbolic tan | `tanh(1)` | kết quả số |
| `degrees(rad)` | Đổi radian sang độ | `degrees(pi)` | `180` |
| `radians(deg)` | Đổi độ sang radian | `radians(180)` | khoảng `3.14159` |

### Logarit và Số mũ

| Hàm | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `ln(a)` | Logarit tự nhiên | `ln(e)` | `1` |
| `log(a)` | Logarit tự nhiên | `log(e)` | `1` |
| `log10(a)` | Logarit cơ số 10 | `log10(1000)` | `3` |
| `exp(a)` | `e^a` | `exp(1)` | khoảng `2.71828` |

### Xử lý Văn bản

| Hàm | Ý nghĩa | Ví dụ | Kết quả |
| :--- | :--- | :--- | :--- |
| `contains(a, b)` | Kiểm tra văn bản `a` có chứa văn bản `b` không (hỗ trợ số, chữ hoặc kết hợp) | 1. `contains(xin chao, chao)` (chữ)<br>2. `contains(Vang: 5000, 5000)` (kết hợp)<br>3. `contains(12345, 99)` (số) | 1. `1` (đúng)<br>2. `1` (đúng)<br>3. `0` (sai) |
| `concat(a, b, ...)` | Nối nhiều giá trị thành một chuỗi văn bản | `concat(NguoiChoi, "-", 01)` | `NguoiChoi-01` |
| `substr(text, start, len)` | Cắt một đoạn chuỗi (hỗ trợ số, chữ hoặc kết hợp) | 1. `substr(chuoi, 2, 3)` (chữ)<br>2. `substr(Hang #1: Player, 9, 6)` (kết hợp)<br>3. `substr(123456, 1, 4)` (số) | 1. `nan`<br>2. `Player`<br>3. `2345` |
| `charat(text, index)` | Lấy một ký tự tại vị trí bắt đầu từ 0; trả về chuỗi rỗng nếu ngoài phạm vi | `charat("hello", 1)` | `e` |
| `len(text)` | Đếm số ký tự (hỗ trợ số, chữ hoặc kết hợp) | 1. `len(apple)` (chữ)<br>2. `len(Diem: 9999)` (kết hợp)<br>3. `len(453454)` (số) | 1. `5`<br>2. `10`<br>3. `6` |
| `lower(text)` | Chuyển văn bản thành chữ thường | `lower(HeLLo)` | `hello` |
| `upper(text)` | Chuyển văn bản thành chữ hoa | `upper(HeLLo)` | `HELLO` |
| `trim(text)` | Xóa khoảng trắng ở đầu và cuối chuỗi | `trim("  hello  ")` | `hello` |
| `myVar.toNumber` | Trích xuất các chữ số từ biến văn bản và chuyển thành dạng số (bỏ qua ký tự không phải số) | Nếu biến `A` là `"Vang: 500"` (văn bản):<br>`A.toNumber` | `500` (dạng số) |
| `myVar.toString` | Chuyển biến thành văn bản bằng cách lọc bỏ toàn bộ chữ số (chỉ giữ lại ký tự chữ) | Nếu biến `A` là `"Dot #10"` (văn bản):<br>`A.toString` | `"Dot #"` (văn bản) |

Các phép so sánh chuỗi bảo toàn khoảng trắng và ký hiệu khi đặt trong dấu ngoặc kép. Ví dụ, `charat(a, 0) == " "` sẽ trả về `1` nếu ký tự đầu tiên là một khoảng trắng.

Tên biến hỗ trợ các phần tính toán lồng `{}`. Gán `item[{i}]` khi `i = 3` sẽ ghi vào `item[3]`; `item[{len(text)}]` cũng hoạt động tương tự. Có thể kết hợp với vòng lặp để tạo `item[1]`, `item[2]`, v.v.

### Biến Có sẵn (Dạng Số)

| Biến số | Ý nghĩa | Ví dụ / Ghi chú |
| :--- | :--- | :--- |
| `screen.width` | Chiều rộng của màn hình chính theo pixel | `screen.width` |
| `screen.height` | Chiều cao của màn hình chính theo pixel | `screen.height` |
| `mouse.x` | Tọa độ X hiện tại của con trỏ chuột | `mouse.x` |
| `mouse.y` | Tọa độ Y hiện tại của con trỏ chuột | `mouse.y` |
| `mouse.sensitivity` | Tốc độ nhạy chuột hiện tại của hệ thống | `mouse.sensitivity` |
| `volume.level` | Mức âm lượng hiện tại của hệ thống (0 đến 100) | `volume.level` |
| `window.x` / `window.left` | Tọa độ X cạnh trái của cửa sổ mục tiêu | `window.x` |
| `window.y` / `window.top` | Tọa độ Y cạnh trên của cửa sổ mục tiêu | `window.y` |
| `window.right` | Tọa độ X cạnh phải của cửa sổ mục tiêu | `window.right` |
| `window.bottom` | Tọa độ Y cạnh dưới của cửa sổ mục tiêu | `window.bottom` |
| `window.width` | Chiều rộng của cửa sổ mục tiêu | `window.width` |
| `window.height` | Chiều cao của cửa sổ mục tiêu | `window.height` |
| `window.centerX` | Tọa độ X tâm điểm của cửa sổ mục tiêu | `window.centerX` |
| `window.centerY` | Tọa độ Y tâm điểm của cửa sổ mục tiêu | `window.centerY` |

### Biến Có sẵn (Hệ thống và Văn bản)

| Biến / Thuộc tính | Ý nghĩa | Ví dụ / Ghi chú |
| :--- | :--- | :--- |
| `system.year` / `month` / `day` | Năm, tháng hoặc ngày hiện tại theo lịch hệ thống | `system.year` |
| `system.hour` / `minute` / `second` | Giờ, phút hoặc giây hiện tại của hệ thống | `system.hour` |
| `system.millisecond` | Phần nghìn giây (millisecond) hiện tại của hệ thống | `system.millisecond` |
| `system.date` | Ngày hiện tại của hệ thống | ví dụ `2026-07-09` |
| `system.time` | Giờ hiện tại của hệ thống | ví dụ `04:24:00` |
| `window.title` | Tiêu đề văn bản của cửa sổ mục tiêu | `window.title` |
| `clipboard.text` | Nội dung văn bản hiện tại trong clipboard | `clipboard.text` |

### Ghi chú

- Các trường biểu thức sẽ tính toán trực tiếp biến số và hàm số.
- Các trường văn bản thuần sẽ giữ nguyên chữ; sử dụng `{...}` để chèn biến số hoặc phép tính vào trong văn bản.
- Toán tử so sánh trả về `1` nếu đúng và `0` nếu sai.
- Một số trường trong macro lưu giá trị cuối cùng dưới dạng số nguyên, nên kết quả số thập phân có thể bị làm tròn.
- Mọi lỗi tính toán hoặc chia cho số 0 (ví dụ `5/0`) đều sẽ an toàn trả về `0`.

</details>

## Bắt đầu Sử dụng

### Yêu cầu Hệ thống

| Yêu cầu | Tối thiểu |
| :--- | :--- |
| Hệ điều hành | Windows 10 / 11 (64-bit) |
| Môi trường chạy | Không cần cài đặt, file `.exe` chạy ngay |
| Quyền thực thi | Quyền Quản trị viên (Administrator) |

### Cài đặt

1. Tải về **`MacroNest.exe`** từ [bản phát hành mới nhất](https://github.com/LinhAsia/MacroNest/releases/latest).
2. Chạy file ứng dụng.

### Tải bổ sung Tùy chọn

Các thành phần này có thể tải trực tiếp trong cài đặt của ứng dụng:

- DLL OpenCV cho tìm kiếm hình ảnh
- Driver Interception cho nhập liệu chuột và bàn phím cấp thấp
- Firmware Arduino cho mô phỏng nhập liệu phần cứng
- Dữ liệu OCR cho nhận diện văn bản (OCR)

## Giấy phép

Được phát hành dưới Giấy phép MIT. Xem chi tiết tại [LICENSE](LICENSE).
