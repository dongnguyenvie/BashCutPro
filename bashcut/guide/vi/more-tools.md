# Công cụ khác

Các công cụ này nằm trong bảng lệnh (**Cmd+Shift+P**); gõ tên của chúng.

## Polish Voice (làm sạch giọng)

Một núm cho giọng nói sạch. Chọn các clip có lời nói rồi chạy **Polish Voice**. Nó bỏ tiếng ù và tiếng ồn đều, làm dịu
âm "s" gắt, cân mức to nhỏ, chặn đỉnh và chỉnh độ lớn cho lời thoại, tất cả trong một bước hoàn tác. Nhấn **Tab** trước
để đổi `amount` (0 đến 1, mặc định 0.5) hoặc nhắm theo độ lớn của một hồ sơ xuất (`"profile": "tiktok"`).

## Word-by-Word Caption Style (phụ đề từng chữ)

Chữ đang nói sáng lên đúng lúc, kiểu phụ đề video ngắn hay dùng. Sau khi tạo phụ đề từ giọng nói, chạy
**Word-by-Word Caption Style** và nhấn **Tab** để đặt kiểu: màu `highlight` (ví dụ `"#ffd400"`), `pop` (chữ đang nói to
lên bao nhiêu, 1.0–1.6), `bold` (đậm), và `dim` (các chữ sắp tới mờ bao nhiêu, 1 = tắt). Phụ đề được đổi thành tiêu đề
sửa được, trong một bước hoàn tác.

## Mẫu dự án

Dựng một lần, dùng lại với cảnh quay mới. **Save as Template…** lưu dự án kèm các *ô* (clip, nhạc nền hay logo sẽ được
thay); **New Project from Template…** điền các ô bằng tệp mới của bạn và lưu thành dự án mới, giữ nguyên mọi điểm cắt,
hiệu ứng và vị trí phụ đề. Hàng loạt có thể bắt đầu mỗi mục từ một mẫu.

## Review Edit (kiểm tra bản dựng)

**Review Edit** kiểm tra bản dựng như một người dựng cẩn thận trước khi xuất: độ lớn và đỉnh âm thanh, khung đen hoặc đứng
hình, chớp sáng, phụ đề, nhịp cắt. Mỗi lỗi thành một **marker Review** trên timeline ghi rõ sai gì, và kết quả cho biết
bản dựng đã sẵn sàng hay **cần sửa**. Trợ lý chạy nó trước mỗi lần xuất.

## Đo đạc

**Measure Loudness** (LUFS và đỉnh thật của bản mix, một track hay một clip), **Detect Beats** (nhịp và phách của clip
nhạc; có thể thêm marker tại các phách) và **Picture Checks** (đoạn đen, đứng hình, tĩnh lâu và chớp sáng).
