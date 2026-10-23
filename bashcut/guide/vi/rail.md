# Thanh bên

Thanh bên là dải biểu tượng ở mép phải cửa sổ. Mỗi biểu tượng mở một mục trong bảng bên; bấm lại để đóng bảng. Các mục
này cũng có trong **Window ▸ BashCutPro**.

![Cửa sổ BashCutPro: thanh bên ở mép phải, mục Hàng loạt đang mở bên cạnh](../img/window.png)

| Mục | Dùng để | Hướng dẫn |
|---|---|---|
| **Trợ lý** (Agent) | Trò chuyện với trợ lý dựng AI (Claude Code) về dự án này; trợ lý dựng, bạn duyệt từng chặng | [Trợ lý](agent.md) |
| **Hàng loạt** (Batch) | Chạy cùng các bước trên nhiều tệp: mỗi tệp thành một dự án và các bản xuất riêng | [Hàng loạt](batch.md) |
| **Màu phim** (Looks) | 26 màu phim, mỗi màu có một dòng mục đích; xem trước và áp cho các clip đang chọn | [Màu phim](looks.md) |
| **Thư viện** (Library) | **Của tôi**: những phần bạn đã lưu. **Trực tuyến**: nhạc, hiệu ứng âm thanh, ảnh và video stock, biểu tượng, phông chữ có giấy phép mở | [Thư viện](library.md) |
| **Plugin** (Plugins) | Cài plugin (đọc tiếng Việt, dịch phụ đề, ảnh AI, tài nguyên trực tuyến…), nhập khóa, chạy thao tác | bên dưới |
| **Tác vụ** (Runs) | Lượt chạy plugin, cài đặt và kiểm tra kho plugin, với tiến độ, lỗi, kết quả và nhật ký | bên dưới |

**Tách cửa sổ** (Pop out). Nút vuông ở đầu mỗi mục đưa nó ra cửa sổ riêng (tiện khi có màn hình thứ hai);
**Gắn lại** (Dock back) đưa về bảng bên. **Cmd+Alt+Shift+A** nhảy tới mục Trợ lý.

**Chấm trên thanh bên.** Rê chuột lên một biểu tượng để xem tên, công dụng và phím tắt của nó. Chấm xanh trên
**Tác vụ** hoặc **Hàng loạt** nghĩa là đang có việc chạy ở đó; chấm đỏ trên **Tác vụ** nghĩa là có lượt chạy bị lỗi kể
từ lần bạn xem trước. Chấm biến mất khi bạn mở mục đó.

## Bảng lệnh

**Cmd+Shift+P** (hoặc **Window ▸ BashCutPro ▸ Command Palette…**) tìm mọi thứ chỉ với vài chữ: lệnh, màu phim, hồ sơ
xuất, thao tác plugin, mẫu dự án và công thức. **↑ ↓** để chọn, **Enter** để chạy, **Esc** để đóng; **Tab** mở các giá
trị của lệnh cần giá trị, kèm một dòng phía trên ghi các khóa lệnh nhận (`?` đánh dấu khóa không bắt buộc). Xem
[Bắt đầu](getting-started.md#4-tìm-mọi-thứ-bảng-lệnh).

![Bảng lệnh đang tìm "razor"](../img/palette.png)

## Plugin

Plugin là chương trình nhỏ chạy cạnh ứng dụng và đưa kết quả vào dự án như media, phụ đề hay thao tác dựng thông thường.

![Mục Plugin với hai plugin đã cài](../img/plugins.png)

1. Mở **Plugin**: lần đầu, mục này tự kiểm tra kho plugin và liệt kê những gì có sẵn (**Làm mới** để kiểm tra lại; rê
   chuột lên dòng "Có N plugin" để xem danh sách lấy từ đâu).
2. **Cài đặt** (Install) plugin bạn cần (nút hiện **Đang cài…** cho tới khi xong). Phần lớn cần Python 3.9 trở lên trên
   máy.
3. Mở **Cài đặt** (Settings) của plugin để điền tùy chọn. Khóa API được giữ trong kho khóa của hệ điều hành (Keychain
   trên macOS, Secret Service trên Linux), không bao giờ nằm trong tệp. Windows chưa hỗ trợ kho khóa: hãy đặt biến
   môi trường chứa khóa của plugin (tên biến có trong README của plugin) trước khi mở BashCutPro.
4. Mỗi **thao tác** có một biểu mẫu nhỏ; **Chạy** (Run) để bắt đầu. Thao tác sửa dự án đang mở cần ứng dụng được mở kèm
   kênh điều khiển (xem [Khắc phục sự cố](troubleshooting.md)).
5. Thẻ của plugin sau đó cho biết lượt chạy ra sao: **Đang chạy 40 %**, **Xong** hoặc **Lỗi:** kèm lý do. **Mở trong Tác
   vụ** để xem nhật ký; **✕** để ẩn dòng này.

Gỡ plugin cũng xóa luôn các khóa đã lưu của nó.

## Tác vụ

![Mục Tác vụ khi chưa có gì chạy](../img/runs.png)

Tác vụ liệt kê các lượt chạy plugin, cài đặt và kiểm tra kho plugin, mới nhất ở trên. Mỗi dòng có tiến độ và thời gian
(cả khi đang chạy), thông báo kết quả, **Nhật ký** (Log, những gì plugin in ra), **Chép nhật ký** (Copy log) và **Hủy**
(Cancel) khi đang chạy. Lượt chạy thất bại, bị lỗi hoặc hết thời gian hiện đầy đủ lỗi màu đỏ (chọn và chép được) cùng
nút **Chạy lại** (Retry), chạy lại đúng thao tác đó với cùng giá trị. Khi lượt chạy tạo ra tệp, **Mở vị trí** (Reveal)
mở thư mục chứa từng tệp. Trong lúc chạy hàng loạt, một dòng cho biết tiến độ (**Hàng loạt: 3 / 10**) với nút **Mở
Hàng loạt** (Open Batch); các mục của nó nằm trong Hàng loạt, còn lượt của trợ lý nằm trong Trợ lý. **Xóa mục đã xong**
(Clear finished) dọn danh sách. Khi plugin cần giúp (ví dụ dịch mà không có khóa API), lượt chạy ghi **Cần trợ lý** (Needs
the agent): **Gửi cho trợ lý** (Send to agent) chuyển việc sang mục Trợ lý để làm nốt.

Bạn không phải chờ: cứ dựng tiếp trong lúc tác vụ chạy. Khi một lượt chạy kết thúc mà mục Tác vụ đang đóng, một thông
báo nhỏ hiện ở góc dưới bên phải trong vài giây: **✓ … đã xong** hoặc **✕ … bị lỗi**. **Xem** (Show) mở Tác vụ đúng lượt
chạy đó; **✕** ẩn thông báo. Dòng thông báo ở đầu mục Plugin hoặc Tác vụ cũng có **✕**; nó chỉ hiện lại khi có thông
báo mới.
