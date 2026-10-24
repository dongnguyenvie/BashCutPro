# Trợ lý: dựng phim cùng Claude Code hoặc Codex

BashCutPro có thể được một trợ lý AI dựng, bằng chính các lệnh bạn dùng. Trợ lý làm theo các kỹ năng dựng trong bộ kỹ
năng (cắt thô theo câu nói, phụ đề, chỉnh màu theo số đo, độ lớn âm thanh cho từng nền tảng, kiểm tra trước khi
xuất…) và dừng ở mỗi chặng để bạn duyệt.

![Mục Trợ lý (Agent)](../img/agent.png)

## Trong ứng dụng: mục Trợ lý

Bạn cần **Claude Code** đã cài và đăng nhập trên máy (lệnh `claude`). Ứng dụng mở từ Finder hoặc Dock không thấy `PATH`
của cửa sổ dòng lệnh, nên BashCutPro tìm thêm trong các thư mục cài thường gặp (`~/.local/bin`, `~/.claude/local`,
`~/.npm-global/bin`, `~/.bun/bin`, `~/.volta/bin`, `/opt/homebrew/bin`, `/usr/local/bin`; `codex` cũng vậy). Nếu cài ở
chỗ khác, đặt biến môi trường `BASHCUT_CLAUDE` là đường dẫn tới nó.

Trên macOS, lượt trợ lý đầu tiên có thể hỏi *"BashCutPro" muốn truy cập các tệp trong thư mục Màn hình nền* (hoặc Tài
liệu, Tải về) khi dự án nằm ở đó: bấm **Cho phép**. Khi chưa trả lời, trợ lý cứ ở trạng thái *đang chạy* mà không làm gì.

1. **Lưu dự án** trước: cuộc trò chuyện và kế hoạch dựng được giữ cạnh dự án (trong `<dự án>.fcproj.bashcut/`).
2. Mở **Trợ lý** (Agent) trên thanh bên (**Cmd+Alt+Shift+A**). Gõ điều bạn muốn ("cắt hết ừm à, dưới một phút, thêm
   phụ đề tiếng Việt") rồi nhấn **Enter** (**Shift+Enter** để xuống dòng).
3. Câu trả lời hiện dần; các lệnh trợ lý chạy được gom mỗi lệnh một dòng. Mỗi thao tác trợ lý làm là một bước hoàn tác
   bình thường. Tin của bạn nằm trong khung bên phải, câu trả lời của trợ lý nằm dưới tên của nó; rê chuột vào một tin
   (hoặc một khối mã trong câu trả lời) để hiện nút **Sao chép**.
4. Ở mỗi chặng (yêu cầu, kế hoạch, cắt thô, bản nháp) trợ lý hiện một **thẻ duyệt** với việc đã làm và số đo đi kèm:
   **Duyệt** (Approve) để đi tiếp, hoặc viết ghi chú rồi **Yêu cầu sửa** (Ask for changes).
5. **Dừng** (Stop) kết thúc lượt của trợ lý trong vài giây, kể cả khi đang chạy một công cụ lâu; bản ghi hiện
   "Đã dừng.", dự án giữ nguyên như lúc đó, và tin nhắn tiếp theo tiếp tục trong cùng cuộc trò chuyện. Những tin
   bạn xếp hàng mà trợ lý chưa bắt đầu sẽ quay lại ô nhập tin. Công cụ bị dừng giữa chừng hiện **Đã dừng**.
   Mỗi lệnh công cụ là một dòng (biểu tượng, tên, việc đã làm, thời gian chạy); lệnh lỗi hiện **Lỗi** kèm lý do
   ngay bên dưới, và bấm vào dòng để xem chi tiết cùng nút **Sao chép**.
6. Bạn vẫn gõ tiếp được khi trợ lý đang làm: mỗi tin nhắn chờ dưới bản ghi với nhãn **Bạn · đang chờ 1**, **2**, …
   theo đúng thứ tự trợ lý sẽ nhận. Bấm **✕** ở một tin để bỏ nó; trợ lý sẽ không trả lời tin đó. Tin trợ lý đã nhận
   rồi thì vẫn được trả lời (muốn ngừng thì bấm **Dừng**).
7. Khi Claude Code cần chạy thứ chưa được cho phép sẵn (ví dụ một lệnh shell hay ghi một tệp), bản ghi hiện một thẻ
   **Claude Code muốn dùng Bash** kèm lệnh hoặc tệp đó: **Cho phép lần này** (nút tô màu) chạy nó một lần; **Luôn cho phép trong
   dự án này** chạy nó và ghi một quy tắc vào `.claude/settings.local.json` trong thư mục dự án (chỉ dự án này, không
   bao giờ vào cài đặt chung của bạn; chỉ có khi dự án đã được lưu); **Từ chối** không chạy và báo cho trợ lý là bạn
   từ chối, trợ lý sẽ làm tiếp theo cách khác. Trợ lý chờ câu trả lời của bạn; **Dừng** cũng từ chối thẻ đang chờ.
   Các bước trợ lý trong Batch không hỏi: thứ cần cho phép sẽ bị từ chối ngay.

8. Gõ **/** để chọn một kỹ năng hoặc lệnh: danh sách phía trên ô nhập tin hiện những gì Claude Code của bạn có (của
   bộ công cụ BashCutPro là `/bcp:setup`, `/bcp:vlog-plan`, …), khớp nhất lên đầu khi bạn gõ tiếp (`/se` ra
   `/bcp:setup`). **↑/↓** để chọn, **Enter** hoặc **Tab** để chèn vào ô nhập tin, **Esc** để đóng danh sách, hoặc bấm
   thẳng vào một dòng. Danh sách được tải khi bạn gửi tin nhắn đầu tiên trong cuộc trò chuyện; trước đó nó hiện
   *Gửi một tin nhắn để tải danh sách kỹ năng*. Codex không có danh sách này.

**Mới** (New) bắt đầu cuộc trò chuyện mới; các cuộc trước được liệt kê và mở lại được.

### Dùng Claude Code trong terminal

Trong **menu** ở tiêu đề (biểu tượng ba gạch bên phải), **Mở trong Terminal** mở một cửa sổ terminal ở thư mục dự án, chạy đúng chương trình trợ lý mà mục
Trợ lý đang dùng và tiếp tục cuộc trò chuyện của nó (`claude --resume <id>`, hoặc `codex resume <id>`): bạn có đầy đủ
màn hình Claude Code (hỏi quyền, lệnh `/`, lịch sử) trên cùng cuộc trò chuyện. Quay lại mục Trợ lý và bấm **Gửi** để
tiếp tục ở đây. Phiên trong terminal điều khiển cửa sổ đang mở qua công cụ của bộ agent kit; kết nối công cụ riêng của
mục Trợ lý không được chuyển sang, trừ khi chưa cài bộ công cụ (xem bên dưới): khi đó phiên terminal nhận kết nối
công cụ của ứng dụng, nên vẫn điều khiển được cửa sổ đang mở. Để chỉ một bên ghi vào cuộc trò chuyện, mục Trợ lý đóng
phiên của mình trước (dừng lượt đang chạy); bản ghi sau đó hiện "Đã chuyển sang tiếp tục trong Terminal."

Một dòng trạng thái dưới các nút theo dõi phiên trong terminal (chỉ Claude Code): "Terminal: đang làm việc…", "đang chờ bạn
cho phép" (đến khi công cụ đó chạy hoặc lượt kết thúc), "đã xong", "dừng do lỗi", "đã đóng". Trạng thái lấy từ hook của Claude
Code, chỉ truyền cho đúng phiên đó (`--settings <thư mục dữ liệu>/agent/terminal-hooks.json`); cài đặt Claude Code của bạn
không bị đổi. Khi bạn thoát Claude Code trong terminal, bản ghi hiện "Đã quay lại bảng này." và **Gửi** tiếp tục ở đây.
Lần đầu ở một thư mục, Claude Code hỏi bạn có tin thư mục đó không: chọn **Yes, I trust this folder**.

- **Đăng nhập…** mở `claude` trong terminal: gõ `/login` rồi làm theo các bước trong trình duyệt. Bạn đăng nhập bằng
  cách đăng nhập của chính Claude Code (gói Claude hoặc API key); BashCutPro không bao giờ đọc hay lưu nó.
- Khi tin nhắn không gửi được vì Claude Code chưa đăng nhập, bảng hiện dòng **Claude Code chưa đăng nhập** dưới phần
  đầu, kèm nút **Đăng nhập…** và thông báo gốc của Claude Code bên dưới (bạn có thể chọn và sao chép). Vấn đề thanh
  toán hoặc tổ chức của tài khoản Claude hiện là **gặp vấn đề tài khoản**. Đăng nhập xong, gửi lại tin nhắn: dòng này
  biến mất khi có tin nhắn mới.
- Khi chưa cài Claude Code, dòng "không tìm thấy" có nút **Cài đặt**. Nút này mở terminal chạy trình cài đặt chính
  thức (`curl -fsSL https://claude.ai/install.sh | bash` trên macOS và Linux, `irm https://claude.ai/install.ps1 | iex`
  trong PowerShell trên Windows). Không có gì được cài cho đến khi bạn bấm.
- Khi BashCutPro không tìm thấy bản bộ công cụ BashCutPro (plugin `bcp`, mang theo các công thức `/bcp:` và kỹ năng) và
  Claude Code chạy mà thiếu nó, sau tin nhắn đầu tiên bảng hiện dòng **Chưa tải kỹ năng BashCutPro**, kèm nút
  **Tải bộ công cụ…**. Nút này mở terminal để tải (clone) bộ công cụ
  (`https://github.com/dongnguyenvie/bashcutpro-agent-kit`) về thư mục dữ liệu của BashCutPro (`bashcut-kit`), hoặc
  chạy `git pull` nếu đã có. Không có gì được tải cho đến khi bạn bấm, và không có gì được cài vào chính Claude Code.
  Tải xong, tin nhắn kế tiếp của bạn sẽ nạp bộ công cụ. Máy không có git thì bảng hiện địa chỉ file zip và thư mục để
  giải nén vào. Dòng ghi chú bên dưới có lệnh tùy chọn nếu bạn muốn có kỹ năng trong mọi phiên Claude Code:
  `claude plugin marketplace add dongnguyenvie/bashcutpro-agent-kit && claude plugin install
  bcp@bashcutpro-agent-kit`. Các nút công thức của cuộc trò chuyện trống cũng cần bộ công cụ.
- Cuộc trò chuyện trống hỏi "Chỉnh sửa gì đây?" và gợi ý vài nút mẫu (Cắt các đoạn im lặng, Thêm phụ đề cho sequence chính, cùng các công thức của bộ kit). Bấm nút chỉ điền vào ô nhắn, chưa gửi gì cho đến khi bạn bấm Gửi.
- Bảng lệnh cũng có ba thao tác này: **Mở trợ lý trong Terminal**, **Đăng nhập Claude Code** và **Cài Claude Code**
  (trợ lý điều khiển BashCutPro có thể gọi chúng bằng `bashcut.agent.terminal`, `bashcut.agent.signIn` và
  `bashcut.agent.install`). Việc cài vẫn chỉ chạy khi bạn chọn. **Tải bộ công cụ BashCutPro**
  (`bashcut.agent.installKit`) làm đúng như nút **Tải bộ công cụ…**; lệnh sẽ báo lỗi nếu cuộc trò chuyện đã thấy bộ
  công cụ.

Terminal là Terminal trên macOS, Windows Terminal (không có thì một cửa sổ PowerShell) trên Windows, và terminal mặc
định của màn hình nền (không có thì GNOME Terminal, Konsole hoặc xterm) trên Linux.

## Codex

Codex dùng được như trợ lý bên ngoài (bên dưới) và trong các lượt hàng loạt, nơi bước trợ lý có thể dùng `codex` thay
cho Claude Code. Mục Trợ lý trong ứng dụng có cả hai: chọn Claude Code hoặc Codex trong danh sách trợ lý. Trợ lý chưa
cài hiện "(chưa cài)" ở đó, kèm một dòng dưới tiêu đề chỉ cách cài hoặc biến cần đặt (`BASHCUT_CLAUDE`,
`BASHCUT_CODEX`).

## Trợ lý trong cửa sổ dòng lệnh

Claude Code hoặc Codex chạy trong cửa sổ dòng lệnh cũng điều khiển được cửa sổ đang mở:

1. Cài bộ kỹ năng trợ lý (bước thiết lập lần đầu có đề nghị), bộ này đăng ký công cụ của BashCutPro với trợ lý.
2. Mở BashCutPro như bình thường. Kênh điều khiển tự bật (chỉ trong máy này, 127.0.0.1) và cửa sổ báo cho trợ lý biết
   nơi tìm nó. Muốn chặn trợ lý, bỏ chọn **Cho phép trợ lý AI điều khiển BashCutPro** trong mục Trợ lý (áp dụng ở lần
   mở sau), hoặc đặt `BASHCUT_CONTROL=0`. `--control <port>` vẫn dùng được để chọn cổng cố định.
   Kênh điều khiển được bảo vệ bằng mã phiên BashCutPro tạo mỗi lần mở; agent mở từ BashCutPro tự nhận mã này.
3. Ra lệnh cho trợ lý trong cửa sổ dòng lệnh; nó tự kết nối vào cửa sổ đang mở. Khi không có cửa sổ nào mở, nó làm việc
   thẳng trên tệp dự án, và từ chối mở một dự án mà cửa sổ đang mở.

Trợ lý chỉ được ghi trong thư mục của dự án (và các thư mục bạn cho phép), nên một nhầm lẫn không thể ghi đè tệp khác
của bạn.
