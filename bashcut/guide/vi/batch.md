# Hàng loạt: nhiều tệp cùng lúc

Chạy hàng loạt áp cùng các bước cho nhiều tệp. **Mỗi tệp thành một dự án riêng**, các tệp được xử lý song song ở chế độ
nền (ứng dụng vẫn rảnh tay, và việc vẫn chạy tiếp kể cả khi bạn đóng ứng dụng), mỗi bước xong đều được lưu, nên hàng
loạt bị dừng sẽ chạy tiếp từ chỗ cũ.

![Mục Hàng loạt (Batch)](../img/batch.png)

## Tạo một lượt hàng loạt

Mở **Hàng loạt** (Batch) trên thanh bên (hoặc **Chạy trên nhiều tệp…** từ một thao tác plugin hay từ trợ lý).

1. **Đầu vào** (Inputs): thả tệp hoặc thư mục vào bảng, hoặc dùng **Thêm tệp…** / **Thêm thư mục…**. Video, âm thanh,
   ảnh và dự án có sẵn (`.fcproj`) đều được. Dự án bạn thêm vào được sao chép, không bao giờ bị sửa.
2. **Các bước** (Steps): **＋ Thêm bước** mở ô tìm lệnh, thao tác plugin và công thức (giống bảng lệnh). Các bước chạy
   lần lượt trên từng dự án.
3. **Đầu ra** (Outputs): **＋ Thêm đầu ra** rồi chọn một **Hồ sơ xuất** (TikTok, Reels, Shorts, YouTube, YouTube 4K,
   Facebook vuông; xem [Hồ sơ xuất](export.md)) hoặc một **Thiết lập xuất** của bạn. Không có đầu ra cũng được: khi đó
   kết quả là các dự án.
4. Đặt **Tên**, chọn số mục chạy **Cùng lúc**, và **Khi một mục lỗi**: **tiếp tục** với các mục khác hay **dừng**. Dòng
   dự án cho biết dự án được lưu ở đâu (mặc định cạnh đầu vào đầu tiên, trong `batch/`; **Chọn…** để đổi).
5. **Chạy hàng loạt** (Run batch).

**Lưu thành mẫu** (Save as preset) giữ toàn bộ thiết lập cho lần sau; **Mẫu thiết lập** (Presets) liệt kê chúng.

## Mẫu có sẵn

Năm mẫu đi kèm bộ plugin của BashCutPro:

| Mẫu | Làm gì | Cần |
|---|---|---|
| `vietsub-folder` | giọng nói thành chữ, phụ đề, dịch sang tiếng Việt, che phụ đề cứng có sẵn, kiểu phụ đề; xuất cho TikTok và YouTube | plugin **translate-captions** và một khóa AI (không có khóa thì mục đó chờ trợ lý) |
| `podcast-shorts` | tìm 3 câu nói hay, mỗi câu làm một video dọc ngắn có phụ đề từng chữ | trợ lý (Claude Code) và bộ kỹ năng trợ lý |
| `every-platform` | mỗi khung hình một bản (YouTube, TikTok, Reels, Shorts, Facebook vuông), phụ đề nằm trong vùng an toàn của từng nền tảng | trợ lý và bộ kỹ năng |
| `captions-draft` | giọng nói thành chữ, phụ đề kiểu chữ gọn, một tệp `.srt` và bản xem thử YouTube | – |
| `clean-audio` | làm sạch giọng và chỉnh độ lớn về −16 LUFS, một tệp `.wav` sạch | – |

Trong lúc ứng dụng chưa tự cài được, hãy thêm chúng một lần bằng cửa sổ dòng lệnh (thư mục nằm trong bản tải
`bashcutpro-plugins`): `bashcut-batch presets install bashcutpro-plugins/packs/batch`. Sau đó chúng hiện trong **Mẫu thiết
lập**.

## Theo dõi

**Hàng đợi** (Queue) có một dòng cho mỗi mục với thanh tiến độ theo bước; nó học thời gian mỗi bước nên ước tính ngày
càng sát. Dòng tóm tắt đếm số mục *xong*, *lỗi*, *cần xem lại* và *còn lại*. Trạng thái của mục:

| Trạng thái | Nghĩa là |
|---|---|
| Đang chờ (Waiting) | chưa bắt đầu (hàng loạt cũng chờ khi thiếu bộ nhớ hay ổ đĩa, rồi tự chạy tiếp) |
| Đang chạy (Running) | đang làm một bước |
| Xong (Done) | mọi bước và bản xuất đã xong |
| Thất bại (Failed) | một bước lỗi; thông báo cho biết vì sao |
| **Cần xem lại** (Needs attention) | cần người (hoặc trợ lý) xem: bước kiểm tra tìm ra lỗi mà trợ lý không sửa được (**cần sửa**), một plugin chuyển mục này lại, hoặc hàng loạt dừng giữa chừng một bước |
| Chưa chạy / Đã dừng / Bị gián đoạn | hàng loạt bị dừng hoặc hủy trước khi mục này xong |

Trong lúc chạy, máy được giữ không ngủ và không bao giờ chạy nhiều mục hơn bộ nhớ cho phép.

## Sửa một mục cần sửa

Khi lượt hàng loạt có bước trợ lý, phần việc của trợ lý được kiểm tra (âm thanh, hình, phụ đề, nhịp cắt). Lỗi nào trợ lý
không sửa được sau hai lần thử sẽ để mục đó ở trạng thái **cần xem lại**.

1. Bấm **Mở để sửa** (Open to fix): dự án của mục đó được mở.
2. Các lỗi nằm trên timeline dưới dạng **marker Review**, mỗi marker có ghi chú nói sai gì và ở đâu (mở
   **Window ▸ Markers** để xem thành danh sách).
3. Sửa trong trình dựng rồi **lưu** dự án.
4. Quay lại **Hàng loạt**, bấm **Chạy lại** (Retry) trên mục đó (hoặc **Chạy lại mục lỗi** cho tất cả). Chỉ các bước
   chưa xong được chạy lại.

**Mở vị trí** (Reveal) mở tệp của một mục trong Finder / Explorer; **Mở thư mục** (Reveal folder) mở cả lượt hàng loạt.
