# Khắc phục sự cố

## Trước tiên: chạy kiểm tra máy

Mở cửa sổ dòng lệnh và chạy `bashcut-doctor`. Nó kiểm tra bộ máy dòng lệnh của trình dựng, bộ kỹ năng trợ lý, Python
của từng plugin, các mô hình đã tải (kích thước và mã kiểm), Claude Code và Codex, dung lượng ổ đĩa và bộ nhớ, và chỉ
cách sửa từng lỗi. `bashcut-doctor --updates` còn báo khi plugin, bộ kỹ năng hoặc BashCutPro có bản mới (không bao giờ tự
cài). Hàng loạt chạy nó trước khi bắt đầu.

## Lỗi thường gặp

| Bạn thấy | Cách xử lý |
|---|---|
| Mục Plugin ghi *Kênh điều khiển đang tắt*, nút **Chạy** của thao tác sửa dự án bị mờ kèm *cần kênh điều khiển*, hoặc thao tác báo *needs the open project* | Kênh này bật sẵn, nên nó đã bị tắt: chọn **Cho phép trợ lý AI điều khiển BashCutPro** trong mục Trợ lý (hoặc bấm **Bật**) rồi khởi động lại BashCutPro; nếu có đặt `BASHCUT_CONTROL=0` thì bỏ đi. `--control 9876` cũng bật nó cho một lần mở. Tìm kiếm, tải về và các thao tác không sửa dự án vẫn chạy được khi không có nó. |
| Plugin không chạy, hoặc báo *Không tìm thấy Python 3* | Plugin cần Python 3.9 trở lên (`bashcut-doctor` nêu tên plugin). Cài Python rồi chạy lại thao tác. |
| Lượt chạy báo *Không tìm thấy bộ máy BashCutPro (bashcutpro-cli) cạnh ứng dụng* | Plugin sửa dự án qua bộ máy nằm cạnh ứng dụng. Cài lại BashCutPro, hoặc build theo [Bắt đầu](getting-started.md), hoặc trỏ `BASHCUTPRO_CLI` tới nó. |
| Lượt chạy ghi **Cần trợ lý** | Plugin không tự làm xong (ví dụ dịch mà không có khóa API). Bấm **Gửi cho trợ lý** trong Tác vụ, hoặc thêm khóa trong **Cài đặt** của plugin rồi chạy lại. |
| Mục Trợ lý không làm gì, hoặc báo *Không tìm thấy Claude Code* | Cài Claude Code và đăng nhập (`claude` trong cửa sổ dòng lệnh). Lưu dự án trước. Cài ngoài các thư mục thường gặp? Đặt `BASHCUT_CLAUDE` (xem [Trợ lý](agent.md#trong-ứng-dụng-mục-trợ-lý)). |
| Trên macOS trợ lý cứ *đang chạy* mà không làm gì | macOS đang chờ hỏi *"BashCutPro" muốn truy cập các tệp trong thư mục …*, có thể bị cửa sổ khác che: bấm **Cho phép**. Sau 30 giây không có phản hồi, mục Trợ lý sẽ báo dưới dòng *Đang làm…*. |
| Một mục hàng loạt **cần xem lại** | Xem [Sửa một mục cần sửa](batch.md#sửa-một-mục-cần-sửa). Mục bị dừng giữa chừng một bước (máy ngủ hoặc treo) cũng nằm ở đây: mở, kiểm tra, **Chạy lại**. |
| Hàng loạt đứng chờ, không chạy mục mới | Máy thiếu bộ nhớ hoặc ổ đĩa; nó tự chạy tiếp khi đủ chỗ. Dọn bớt ổ đĩa, hoặc giảm số **Cùng lúc**. |
| Thư viện ▸ Trực tuyến ghi *Chưa rõ giấy phép* | Nguồn không ghi giấy phép. Đừng đăng tệp đó khi chưa biết rõ. |
| Mất thanh bên | **Window ▸ BashCutPro ▸ Hiện thanh bên**. |
| Phím tắt không có tác dụng | Xem **Edit ▸ Keyboard Shortcuts**; bảng lệnh là **Cmd+Shift+P** (Cmd+K là Add Edit của FilmCraft). |

## BashCutPro lưu mọi thứ ở đâu

Trong thư mục dữ liệu riêng của BashCutPro (`~/Library/Application Support/BashCutPro` trên Mac, `%APPDATA%\BashCutPro`
trên Windows, `~/.local/share/bashcutpro` trên Linux; bản cài di động dùng thư mục `data` cạnh ứng dụng). Đây không
phải thư mục của FilmCraft, nên có thể cài BashCutPro và FilmCraft song song mà không dùng chung thiết lập. Lần mở
đầu tiên, BashCutPro chép một lần thiết lập, dự án gần đây, mẫu xuất, hồ sơ, mẫu hàng loạt và plugin từ FilmCraft
(danh sách nằm trong `bashcut/imported-from-filmcraft.txt`); mô hình giọng nói không được chép mà dùng thẳng từ thư
mục của FilmCraft cho đến khi BashCutPro có thư mục `models` riêng:

- `bashcut/`: hồ sơ xuất, mẫu dự án, màu phim, mẫu hàng loạt và thiết lập của bảng bên;
- `bashcut-plugins/`: plugin đã cài và tệp chúng tải về;
- cạnh mỗi dự án, `<dự án>.fcproj.bashcut/`: cuộc trò chuyện với trợ lý, kế hoạch dựng và kết quả plugin.

Dự án làm bằng BashCutPro vẫn mở được trong FilmCraft gốc; FilmCraft chỉ bỏ qua các phần thêm này.
