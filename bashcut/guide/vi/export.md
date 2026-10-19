# Hồ sơ xuất: bản xuất đúng cho từng nền tảng

Hồ sơ xuất là mọi thứ một nền tảng cần ở video: kích thước và tốc độ khung hình, codec, bitrate theo hướng dẫn tải lên
của chính nền tảng, độ lớn âm thanh, phụ đề (in cứng hoặc tệp rời) và vùng an toàn mà các nút của nền tảng che mất.

| Hồ sơ | Khung | Dùng cho |
|---|---|---|
| **TikTok**, **Reels**, **Shorts** | 1080×1920 dọc | video dọc ngắn |
| **YouTube** | 1920×1080 | video ngang |
| **YouTube 4K** | 3840×2160 | video ngang 4K |
| **Facebook square** | 1080×1080 | bài vuông |

## Xuất với một hồ sơ

Bước thiết lập lần đầu biến mỗi hồ sơ thành một thiết lập xuất tên **BashCut** + tên hồ sơ (ví dụ **BashCut TikTok**).
Nếu bạn đã bỏ qua bước đó, mở bảng lệnh (**Cmd+Shift+P**) và chạy **Compile Output Profiles to Export Presets** một lần.

Sau đó xuất như bình thường trong FilmCraft (**File ▸ Export ▸ Media…**, chế độ Export, hàng đợi xuất hoặc Quick Export)
và chọn thiết lập **BashCut …**. Thế là xong: kích thước, bitrate và độ lớn âm thanh đã đúng cho nền tảng.

Khi chạy hàng loạt, chọn hồ sơ ở **Đầu ra** ([Hàng loạt](batch.md)). Bảng lệnh cũng có **Export with Output Profile**
(nhấn Tab để nhập hồ sơ và tệp cần ghi).

## Hồ sơ của riêng bạn

Một hồ sơ là một tệp JSON nhỏ. Đặt tệp của bạn trong `<thư mục dữ liệu>/bashcut/profiles/` (thư mục dữ liệu của
FilmCraft: `~/Library/Application Support/FilmCraft` trên Mac, `%APPDATA%\FilmCraft` trên Windows,
`~/.local/share/filmcraft` trên Linux). Tệp có cùng `id` với một hồ sơ có sẵn sẽ thay hồ sơ đó. Sau đó chạy lại
**Compile Output Profiles to Export Presets**.
