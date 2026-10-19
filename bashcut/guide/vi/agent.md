# Trợ lý: dựng phim cùng Claude Code hoặc Codex

BashCutPro có thể được một trợ lý AI dựng, bằng chính các lệnh bạn dùng. Trợ lý làm theo các kỹ năng dựng trong bộ kỹ
năng (cắt thô theo câu nói, phụ đề, chỉnh màu theo số đo, độ lớn âm thanh cho từng nền tảng, kiểm tra trước khi
xuất…) và dừng ở mỗi chặng để bạn duyệt.

## Trong ứng dụng: mục Trợ lý

Bạn cần **Claude Code** đã cài và đăng nhập trên máy (lệnh `claude`). Nếu cài ở chỗ lạ, đặt biến môi trường
`BASHCUT_CLAUDE` là đường dẫn tới nó.

1. **Lưu dự án** trước: cuộc trò chuyện và kế hoạch dựng được giữ cạnh dự án (trong `<dự án>.fcproj.bashcut/`).
2. Mở **Trợ lý** (Agent) trên thanh bên (**Cmd+Alt+Shift+A**). Gõ điều bạn muốn ("cắt hết ừm à, dưới một phút, thêm
   phụ đề tiếng Việt") rồi nhấn **Enter** (**Shift+Enter** để xuống dòng).
3. Câu trả lời hiện dần; các lệnh trợ lý chạy được gom mỗi lệnh một dòng. Mỗi thao tác trợ lý làm là một bước hoàn tác
   bình thường.
4. Ở mỗi chặng (yêu cầu, kế hoạch, cắt thô, bản nháp) trợ lý hiện một **thẻ duyệt** với việc đã làm và số đo đi kèm:
   **Duyệt** (Approve) để đi tiếp, hoặc viết ghi chú rồi **Yêu cầu sửa** (Ask for changes).
5. **Dừng** (Stop) kết thúc lượt của trợ lý ngay; dự án giữ nguyên như lúc đó.

**Mới** (New) bắt đầu cuộc trò chuyện mới; các cuộc trước được liệt kê và mở lại được.

## Codex

Codex dùng được như trợ lý bên ngoài (bên dưới) và trong các lượt hàng loạt, nơi bước trợ lý có thể dùng `codex` thay
cho Claude Code. Mục Trợ lý trong ứng dụng hiện dùng Claude Code.

## Trợ lý trong cửa sổ dòng lệnh

Claude Code hoặc Codex chạy trong cửa sổ dòng lệnh cũng điều khiển được cửa sổ đang mở:

1. Cài bộ kỹ năng trợ lý (bước thiết lập lần đầu có đề nghị), bộ này đăng ký công cụ của BashCutPro với trợ lý.
2. Mở BashCutPro kèm kênh điều khiển: thêm `--control 9876` khi mở (trên Mac:
   `open -a BashCutPro --args --control 9876`), hoặc đặt `FILMCRAFT_CONTROL_PORT=9876` trước khi mở. Cửa sổ sẽ báo cho
   trợ lý biết nơi tìm nó.
3. Ra lệnh cho trợ lý trong cửa sổ dòng lệnh; nó tự kết nối vào cửa sổ đang mở. Khi không có cửa sổ nào mở, nó làm việc
   thẳng trên tệp dự án, và từ chối mở một dự án mà cửa sổ đang mở.

Trợ lý chỉ được ghi trong thư mục của dự án (và các thư mục bạn cho phép), nên một nhầm lẫn không thể ghi đè tệp khác
của bạn.
