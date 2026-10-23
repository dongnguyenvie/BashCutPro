# BashCutPro

BashCutPro is a video editor for people who cut with AI agents, plugins and batches. It is built on the open-source
FilmCraft editor (MIT / Apache-2.0, see `NOTICE`) and adds an Agent, Plugins, Runs and Batch side panel, plus
command-line tools that work on the open window.

## Install and run

- **Windows**: run the `.msi` installer, then open BashCutPro from the Start menu. The portable zip needs no install:
  unzip it anywhere and run `bashcutpro.exe`.
- **macOS**: open the `.dmg` and drag BashCutPro to Applications.
- **Linux**: install the `.deb`, `.rpm`, AppImage or Flatpak, or unpack the tarball and run `./install.sh`. Then run
  `bashcutpro`.

First run (optional, once): the setup script installs the looks and output profiles, connects an AI agent and checks
your setup. Windows: `powershell -File bashcut-setup.ps1` in the install folder; macOS:
`bash BashCutPro.app/Contents/Resources/bashcut/bashcut-setup.sh`; Linux: `bash /usr/share/bashcutpro/bashcut-setup.sh`.
`bashcut-doctor` checks your setup again at any time.

## Command line

- `bashcutpro-cli` (short name `bcpro`): the editor's engine. While BashCutPro is open, `bcpro exec …`, `run`,
  `inspect` and `mcp` work on the open window; add `--project <file>` to work on a project file without it.
- `bashcut-batch` runs batches, `bashcut-mcp` lets an AI agent reach the open window, `bashcut-doctor` checks your
  setup. Each one answers `--help`.

## Your data

Preferences, presets, plugins, logs (`Logs/bashcutpro.log`) and crash recovery live in:

- Windows: `%APPDATA%\BashCutPro` (portable zip: the `data` folder next to `bashcutpro.exe`)
- macOS: `~/Library/Application Support/BashCutPro`
- Linux: `~/.local/share/bashcutpro`

`BASHCUTPRO_DATA_DIR` moves it. Render previews go in a `BashCutPro Previews` folder next to each saved project.

## More

Guide, issues and source: https://github.com/dongnguyenvie/BashCutPro

---

# BashCutPro (Tiếng Việt)

BashCutPro là phần mềm dựng video dành cho người làm việc cùng agent AI, plugin và xử lý hàng loạt. Ứng dụng được
xây dựng trên trình dựng mã nguồn mở FilmCraft (MIT / Apache-2.0, xem `NOTICE`), có thêm bảng bên Agent, Plugin,
Lượt chạy và Hàng loạt, cùng các công cụ dòng lệnh làm việc trực tiếp trên cửa sổ đang mở.

## Cài đặt và chạy

- **Windows**: chạy bộ cài `.msi`, rồi mở BashCutPro từ menu Start. Bản zip portable không cần cài: giải nén ở bất
  kỳ đâu và chạy `bashcutpro.exe`.
- **macOS**: mở file `.dmg` và kéo BashCutPro vào Applications.
- **Linux**: cài gói `.deb`, `.rpm`, AppImage hoặc Flatpak, hoặc giải nén file tar rồi chạy `./install.sh`. Sau đó
  chạy `bashcutpro`.

Lần chạy đầu (tùy chọn, chỉ một lần): script cài đặt thêm bộ look và cấu hình xuất, kết nối agent AI và kiểm tra máy.
Windows: `powershell -File bashcut-setup.ps1` trong thư mục cài đặt; macOS:
`bash BashCutPro.app/Contents/Resources/bashcut/bashcut-setup.sh`; Linux: `bash /usr/share/bashcutpro/bashcut-setup.sh`.
Có thể chạy `bashcut-doctor` để kiểm tra lại bất cứ lúc nào.

## Dòng lệnh

- `bashcutpro-cli` (tên ngắn `bcpro`): engine của trình dựng. Khi BashCutPro đang mở, `bcpro exec …`, `run`,
  `inspect` và `mcp` làm việc trên cửa sổ đó; thêm `--project <file>` để làm việc với file dự án mà không cần cửa sổ.
- `bashcut-batch` chạy xử lý hàng loạt, `bashcut-mcp` cho agent AI truy cập cửa sổ đang mở, `bashcut-doctor` kiểm
  tra cài đặt. Lệnh nào cũng có `--help`.

## Dữ liệu của bạn

Tùy chọn, preset, plugin, nhật ký (`Logs/bashcutpro.log`) và dữ liệu khôi phục sau sự cố nằm ở:

- Windows: `%APPDATA%\BashCutPro` (bản zip portable: thư mục `data` cạnh `bashcutpro.exe`)
- macOS: `~/Library/Application Support/BashCutPro`
- Linux: `~/.local/share/bashcutpro`

Đặt `BASHCUTPRO_DATA_DIR` để đổi vị trí. Bản xem trước đã render nằm trong thư mục `BashCutPro Previews` cạnh mỗi dự
án đã lưu.

## Thêm thông tin

Hướng dẫn, báo lỗi và mã nguồn: https://github.com/dongnguyenvie/BashCutPro
