# Roadmap nâng cấp — Media Downloader v2

**Ngày lập**: 2026-07-26
**Trạng thái**: Đã chốt phạm vi, chờ triển khai

Tài liệu này chia đợt nâng cấp sau v1 thành 4 phase độc lập. Mỗi phase có một
spec riêng, tự chạy được `/speckit-plan` → `/speckit-tasks` → `/speckit-implement`
mà không phụ thuộc phase sau.

## Bối cảnh

v1 (`specs/001-media-downloader-desktop/`) đã hoàn tất 48/50 task: tải audio/video
qua yt-dlp, fallback gallery-dl cho ảnh/slideshow, hàng đợi có pause/resume/cancel/retry,
lịch sử, cài đặt (theme, ngôn ngữ, thư mục mặc định), trang Logs. Còn nợ T049 (e2e
`tauri-driver`) và T050 (smoke test thủ công 6 nền tảng) — hai việc này **không**
thuộc roadmap v2, xử lý riêng.

## Bốn phase

| Phase | Spec | Nội dung | Vì sao ở vị trí này |
|---|---|---|---|
| 1 | `002-download-power/` | Nhập hàng loạt, kéo-thả, hàng đợi thật sự (sắp xếp lại, song song chỉnh được, sống sót qua khởi động lại), retry có backoff, giới hạn tốc độ, tray + thông báo, link trực tiếp/HLS | Dùng lại hạ tầng queue sẵn có; sửa đúng chỗ đau nhất hiện nay (queue chỉ nằm trong RAM, batch mode bị ép audio) |
| 2 | `003-media-output/` | Nhiều định dạng audio/video, nhúng metadata + thumbnail, template đặt tên file, phụ đề, cắt đoạn, tách chapter, preset tải | Chủ yếu là mở rộng cờ yt-dlp/ffmpeg, không đụng kiến trúc; nhưng cần Phase 1 xong để preset áp được cho batch |
| 3 | `004-library/` | Trang Library, chỉ mục file đã tải, tìm/lọc, player nhúng, quản lý file, thống kê, xuất M3U | Cần bảng DB mới + bật `assetProtocol`; giá trị tăng theo lượng file đã tải nên hợp lý làm sau |
| 4 | `005-toolbox/` | Bộ công cụ ffmpeg cho file có sẵn: chuyển đổi, trích audio, cắt/ghép, nén, tạo GIF, chuẩn hoá âm lượng, xử lý phụ đề, đổi tên hàng loạt | Độc lập hoàn toàn với ba phase trên; nhận input từ Library (Phase 3) nếu có, không thì kéo-thả file |

## Phụ thuộc giữa các phase

```
Phase 1 (queue + intake)
   │
   ├──> Phase 2 (output options)   — preset cần batch của Phase 1 để có ý nghĩa
   │
   └──> Phase 3 (library)          — dùng bảng downloaded_files mà Phase 1/2 ghi đầy đủ hơn
                                        │
                                        └──> Phase 4 (toolbox)  — lấy input từ Library (tuỳ chọn)
```

Phase 4 có thể làm bất cứ lúc nào nếu chấp nhận input chỉ từ kéo-thả file.

## Ngoài phạm vi v2

Nhóm **Vận hành & độ bền** (tự cập nhật yt-dlp/gallery-dl, cookies từ trình duyệt,
proxy, auto-update ứng dụng, phân loại lỗi chi tiết) được **cố ý gác lại** theo
quyết định ngày 2026-07-26. Riêng phần phân loại lỗi mạng vs lỗi nội dung vẫn được
kéo vào Phase 1 vì retry có backoff không thể làm đúng nếu thiếu nó.

Cũng ngoài phạm vi: theo dõi kênh/playlist tự động (subscription), tiện ích mở rộng
trình duyệt, chế độ CLI, và chọn range playlist kiểu `1-10` / `20-30` (gác lại theo
quyết định ngày 2026-07-26 — việc chọn từng mục bằng checkbox trong panel playlist
đã đủ dùng).

**Bị loại vĩnh viễn — theo dõi clipboard**: ý tưởng tự bắt link khi người dùng copy
đã bị bác bỏ ngày 2026-07-26 vì buộc ứng dụng đọc mọi thứ người dùng copy, kể cả
mật khẩu và tin nhắn riêng tư. Đây là ranh giới quyền riêng tư đã chốt, không phải
tính năng hoãn lại. Xem `FR-110b` trong spec Phase 1.

## Nợ kỹ thuật phát hiện khi khảo sát (xử lý lồng vào các phase)

| Vấn đề | Vị trí | Phase xử lý |
|---|---|---|
| `list_queue` đã đăng ký ở Rust nhưng frontend chưa từng gọi → queue mất khi khởi động lại | `src-tauri/src/lib.rs:45`, `src/stores/queue-store.ts` | 1 |
| `enqueue` spawn ngay rồi mới chờ `Semaphore` → không có hàng đợi chờ thật, không sắp xếp lại được | `src-tauri/src/downloader/queue.rs:82-113` | 1 |
| Retry lặp lại cả lỗi vĩnh viễn (video riêng tư, URL không hỗ trợ) 3 lần vô ích | `queue.rs:210, 248` | 1 |
| Job đang `downloading` khi app tắt đột ngột kẹt trạng thái đó vĩnh viễn | không có code reconcile | 1 |
| Pause rồi resume rất nhanh có thể xoá nhầm handle của lần chạy mới (key trùng `job_id`) | `queue.rs:111` vs `:117-129` | 1 |
| Cancel không có tác dụng trong giai đoạn `dump_gallery_json` | `queue.rs:371-402` | 1 |
| Chuỗi tiếng Việt hard-code bỏ qua i18n | `DownloadForm.tsx:476,478,483,501,541`; `History.tsx:18,28,36-39` | 1 |
| `DownloadForm.tsx` 823 dòng gánh 6 trách nhiệm | `src/components/DownloadForm.tsx` | 1 (tách trước khi thêm) |
| Thiếu key `downloadForm.gallery_item_count_other` trong `vi.json`; không có kiểm tra parity locale | `src/locales/` | 1 |
| Gallery grid chỉ render 24 item đầu nhưng selection mặc định chọn tất cả | `DownloadForm.tsx:627` | 1 |
| `tsconfig.json` chỉ `include: ["src"]` → thư mục `tests/` không được type-check | `tsconfig.json:26` | 1 |
| `downloaded_files` được ghi nhưng chưa bao giờ đọc | `db/mod.rs:204-217` | 3 |
| Chưa bundle `ffprobe`; đang parse stderr của `ffmpeg -i` để lấy duration | `queue.rs:609-636` | 4 |
| `@tauri-apps/plugin-fs` và crate `thiserror` là dependency chết | `package.json:22`, `Cargo.toml:29` | 1 (dọn) |
| `app.security.csp: null` — CSP đang tắt hoàn toàn | `tauri.conf.json:26` | 3 (bật khi cấu hình assetProtocol) |
