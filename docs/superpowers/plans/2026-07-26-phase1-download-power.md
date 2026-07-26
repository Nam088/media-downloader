# Phase 1 — Download Power Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Biến hàng đợi tải xuống từ "spawn ngay, giữ trong RAM" thành một bộ điều phối thật sự — có thứ tự, sống sót qua khởi động lại, chỉnh được số luồng lúc đang chạy, retry đúng loại lỗi — và mở luồng nhập liệu hàng loạt (dán nhiều URL, kéo-thả, file `.txt`) với đầy đủ lựa chọn loại nội dung và chất lượng.

**Architecture:** Thay cơ chế `enqueue → tokio::spawn → semaphore.acquire()` bằng một **dispatcher task duy nhất**: `enqueue` chỉ ghi DB (status `queued` + `queue_position`) rồi đánh thức dispatcher; dispatcher tick mỗi giây hoặc khi được đánh thức, đọc job kế tiếp đủ điều kiện từ DB và khởi chạy nếu còn slot. Nhờ đó thứ tự nằm trong DB (sắp xếp lại được, bền qua khởi động lại), số luồng là một `AtomicUsize` đọc lại mỗi vòng (đổi được lúc đang chạy), và **retry trở thành một trạng thái dữ liệu** (`status='queued'` + `next_retry_at` ở tương lai) thay vì một vòng lặp bị kẹt trong task — nên huỷ được và hiển thị được.

**Tech Stack:** Rust + Tauri 2.11 (`tokio` 1.53, `rusqlite` 0.40 + `rusqlite_migration` 2.6), React 19 + TypeScript + zustand + shadcn/ui + Tailwind 4, i18next, Vitest + Testing Library.

---

## Bối cảnh bắt buộc phải đọc trước khi bắt đầu

| Điều cần biết | Vị trí |
|---|---|
| Spec đầy đủ của phase này (FR-101→135, SC-101→111) | `specs/002-download-power/spec.md` |
| Vòng đời job hiện tại | `src-tauri/src/downloader/queue.rs:76-190` |
| Cách build cờ cho yt-dlp | `src-tauri/src/downloader/queue.rs:905-988` |
| Phân loại lỗi hiện tại | `src-tauri/src/downloader/ytdlp.rs:234-251` |
| Migration: **không bao giờ sửa file đã release**, chỉ thêm file mới, và phải thêm cả dòng `M::up(...)` vào `src-tauri/src/db/mod.rs:11-21` | `src-tauri/src/db/migrations/0005_fix_stale_app_settings_schema.sql:1-16` giải thích lý do |
| `row_to_job` đọc theo **tên cột**, nên thêm cột mới là an toàn | `src-tauri/src/db/mod.rs:280-320` |
| Setting mới **không cần migration** — `get_setting_or_default` tự tạo row khi đọc lần đầu | `src-tauri/src/db/mod.rs:225-232` |
| Mẫu test Rust: `#[cfg(test)] mod tests` inline cuối file | `src-tauri/src/downloader/queue.rs:1049` |
| Mẫu test frontend: mock `invoke` theo tên command trong `beforeEach` | `tests/unit/LanguageSwitcher.test.tsx:10-17` |

**Lệnh chạy test:**
- Rust: `cd src-tauri && cargo test`
- Frontend: `pnpm test`
- Kiểm tra kiểu: `pnpm build`

---

## File Structure

### Backend — tạo mới

| File | Trách nhiệm |
|---|---|
| `src-tauri/src/db/migrations/0008_queue_scheduling.sql` | Thêm `queue_position`, `retry_count`, `next_retry_at` vào `download_jobs` + index |
| `src-tauri/src/downloader/scheduler.rs` | Vòng lặp dispatcher, đếm slot, đọc job kế tiếp, đánh thức |
| `src-tauri/src/downloader/retry.rs` | Phân loại lỗi tạm thời/vĩnh viễn + tính khoảng chờ backoff |
| `src-tauri/src/commands/queue_control.rs` | Các lệnh hàng loạt: tạm dừng/tiếp tục/huỷ tất cả, sắp xếp lại thứ tự |
| `src-tauri/src/commands/url_list.rs` | Đọc file `.txt` danh sách URL (giữ quyền đọc tệp ở phía Rust, không mở quyền `fs` cho giao diện) |
| `src-tauri/src/tray.rs` | Biểu tượng khay hệ thống + menu + hành vi đóng cửa sổ |
| `src-tauri/src/notify.rs` | Gửi thông báo hệ thống khi cửa sổ không hiển thị |

### Backend — sửa

| File | Thay đổi |
|---|---|
| `src-tauri/src/db/mod.rs` | Đăng ký migration 0008; fractional indexing (`position_between`, `needs_renormalize`, `move_job_between`, `renormalize_queue_positions`); thêm `next_dispatchable_job`, `next_queue_position`, `reset_interrupted_jobs`, `mark_job_for_retry`, `bulk_update_status`; mở rộng `insert_job`/`row_to_job`; 4 setting mới |
| `src-tauri/src/models.rs` | `DownloadJob` thêm `queue_position`/`retry_count`/`next_retry_at`; `AppSettings` thêm 4 trường |
| `src-tauri/src/downloader/queue.rs` | Bỏ `Semaphore` và vòng `for attempt`; `enqueue` chỉ ghi DB + đánh thức; thêm `start_job`, `set_max_concurrent`; sửa lỗi tranh chấp handle; đưa cancel vào giai đoạn dump gallery; `build_ytdlp_args` nhận giới hạn tốc độ |
| `src-tauri/src/downloader/ytdlp.rs` | `classify_ytdlp_error` nhận thêm nhánh `NETWORK_ERROR` |
| `src-tauri/src/downloader/gallery_dl.rs` | `classify_gallery_dl_error` nhận thêm nhánh `NETWORK_ERROR` |
| `src-tauri/src/commands/settings.rs` | `UpdateSettingsInput` thêm 4 trường; áp số luồng mới lên scheduler |
| `src-tauri/src/lib.rs` | Gọi `reset_interrupted_jobs` lúc khởi động; đăng ký lệnh mới; dựng khay hệ thống; đăng ký plugin thông báo |
| `src-tauri/Cargo.toml` | Thêm `tauri-plugin-notification`; bật feature `tray-icon` cho `tauri` |
| `src-tauri/capabilities/default.json` | Thêm quyền `notification` |

### Frontend — tạo mới

| File | Trách nhiệm |
|---|---|
| `src/lib/format.ts` | `formatDuration`, `formatFileSize`, `formatSpeed`, `formatEta` — gom 3 bản trùng lặp hiện có |
| `src/lib/url-parsing.ts` | `isValidUrl`, `extractUrlsFromText`, `dedupeUrls` |
| `src/lib/build-job-input.ts` | Dựng `CreateJobInput` từ preview + lựa chọn (gom 2 nhánh đang lặp) |
| `src/hooks/use-media-preview.ts` | Trạng thái xem trước một URL + huỷ |
| `src/hooks/use-batch-download.ts` | Xem trước song song có giới hạn + tạo job cho cả lô, kèm trạng thái từng URL |
| `src/hooks/use-file-drop.ts` | Lắng nghe sự kiện thả file/URL vào cửa sổ |
| `src/components/BatchPanel.tsx` | Bảng trạng thái từng URL + chọn loại nội dung và chất lượng cho cả lô |
| `src/components/GalleryItemPicker.tsx` | Lưới chọn ảnh (tách khỏi `DownloadForm`, bỏ giới hạn 24 mục) |
| `src/components/QueueToolbar.tsx` | Thanh thao tác hàng loạt trên hàng đợi |
| `tests/unit/locale-parity.test.ts` | Kiểm tra key dịch khớp nhau giữa các ngôn ngữ |

### Frontend — sửa

| File | Thay đổi |
|---|---|
| `src/components/DownloadForm.tsx` | Rút gọn: dùng các module vừa tách; chế độ lô dùng `BatchPanel`; nhận file/URL thả vào |
| `src/components/QueueList.tsx` | Nạp lại từ backend lúc khởi động; kéo sắp xếp; đếm ngược retry; thanh thao tác hàng loạt |
| `src/stores/queue-store.ts` | `hydrate()` từ `list_queue`; `moveJob()` (fractional); các hành động hàng loạt |
| `src/pages/Settings.tsx` | Số luồng song song, giới hạn tốc độ, chạy nền |
| `src/types/download.ts`, `src/types/settings.ts` | Trường mới khớp Rust |
| `src/locales/en.json`, `src/locales/vi.json` | Chuỗi mới + bù key thiếu |
| `tsconfig.json` | `include` thêm `tests` |
| `src-tauri/tauri.conf.json` | Bật thả tệp vào cửa sổ |

---

## Nhóm A — Nền móng backend (Task 1–9)

### Task 1: Migration 0008 + trường mới trên DownloadJob

**Files:**
- Create: `src-tauri/src/db/migrations/0008_queue_scheduling.sql`
- Modify: `src-tauri/src/db/mod.rs:11-21` (đăng ký migration), `:58-101` (`insert_job`), `:280-320` (`row_to_job`)
- Modify: `src-tauri/src/models.rs:75-130` (`DownloadJob`)
- Test: `src-tauri/src/db/mod.rs` (thêm `#[cfg(test)] mod tests` cuối file)

- [ ] **Step 1: Viết test thất bại**

Thêm vào cuối `src-tauri/src/db/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{JobStatus, MediaType};

    /// Mỗi test dùng một file DB riêng trong thư mục tạm của hệ điều hành để
    /// migration chạy thật (in-memory không kiểm chứng được `to_latest`).
    fn temp_db() -> Db {
        let path = std::env::temp_dir()
            .join(format!("media-downloader-test-{}.db", uuid::Uuid::new_v4()));
        Db::open(&path).expect("db opens")
    }

    fn sample_job(id: &str) -> DownloadJob {
        DownloadJob {
            id: id.to_string(),
            source_url: "https://example.com/v".to_string(),
            platform: "youtube".to_string(),
            media_type: MediaType::Audio,
            audio_quality: Some("128kbps".to_string()),
            video_quality: None,
            gallery_mode: None,
            selected_gallery_indices: None,
            status: JobStatus::Queued,
            progress_percent: 0.0,
            speed_bytes_per_sec: None,
            eta_seconds: None,
            error_message: None,
            output_directory: "/tmp".to_string(),
            output_file_path: None,
            is_playlist_item: false,
            parent_playlist_id: None,
            retried_from_job_id: None,
            created_at: "2026-07-26T00:00:00Z".to_string(),
            updated_at: "2026-07-26T00:00:00Z".to_string(),
            title: None,
            playlist_title: None,
            queue_position: 0.0,
            retry_count: 0,
            next_retry_at: None,
        }
    }

    #[test]
    fn round_trips_scheduling_fields() {
        let db = temp_db();
        let mut job = sample_job("job-1");
        job.queue_position = 7.5;
        job.retry_count = 2;
        job.next_retry_at = Some("2026-07-26T00:00:30Z".to_string());
        db.insert_job(&job).expect("insert works");

        let loaded = db.get_job("job-1").expect("query works").expect("job exists");
        assert_eq!(loaded.queue_position, 7.5);
        assert_eq!(loaded.retry_count, 2);
        assert_eq!(
            loaded.next_retry_at.as_deref(),
            Some("2026-07-26T00:00:30Z")
        );
    }
}
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `cd src-tauri && cargo test round_trips_scheduling_fields`
Expected: FAIL — lỗi biên dịch `struct DownloadJob has no field named queue_position`.

- [ ] **Step 3: Tạo file migration**

Tạo `src-tauri/src/db/migrations/0008_queue_scheduling.sql`:

```sql
-- Phase 1 (specs/002-download-power): hàng đợi chờ có thứ tự thật sự + retry
-- là trạng thái dữ liệu thay vì vòng lặp trong task.
--
-- `queue_position` là REAL chứ không phải INTEGER vì thứ tự dùng *fractional
-- indexing*: kéo một mục vào giữa hai mục khác chỉ ghi đúng MỘT dòng, với giá
-- trị là điểm giữa của hai hàng xóm.
--
-- Lý do quan trọng hơn cả hiệu năng: nếu mỗi lần kéo phải ghi lại vị trí của
-- cả danh sách, thì một job vừa được thêm vào trong lúc người dùng đang kéo sẽ
-- bị ghi đè vị trí — snapshot mà giao diện gửi lên đã cũ. Chỉ đụng một dòng thì
-- không có tranh chấp đó (FR-117, FR-119).
--
-- Khe hở giữa hai vị trí bị chia đôi mỗi lần chèn vào cùng một chỗ; khi nó nhỏ
-- hơn ngưỡng an toàn, `renormalize_queue_positions` đánh số lại 1.0, 2.0, 3.0…
-- Xem `db::position_between` và `db::needs_renormalize`.
--
-- `retry_count` / `next_retry_at`: một job đang chờ thử lại là job có
-- status='queued' và next_retry_at ở tương lai (FR-121, FR-122). Cách này
-- tránh phải thêm giá trị mới vào ràng buộc CHECK trên cột status — SQLite
-- không ALTER được CHECK, sẽ phải rebuild cả bảng.
ALTER TABLE download_jobs ADD COLUMN queue_position REAL NOT NULL DEFAULT 0;
ALTER TABLE download_jobs ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE download_jobs ADD COLUMN next_retry_at TEXT;

-- Job có sẵn đều mang mặc định 0, tức là hoà nhau hết. Đánh số lại theo rowid
-- (xấp xỉ thứ tự tạo) để chúng có vị trí phân biệt ngay từ đầu, thay vì phải
-- dựa vào `created_at` làm tiêu chí phân định mãi mãi.
UPDATE download_jobs SET queue_position = rowid;

CREATE INDEX idx_download_jobs_dispatch
    ON download_jobs (status, queue_position, created_at);
```

- [ ] **Step 4: Đăng ký migration**

Trong `src-tauri/src/db/mod.rs`, thêm dòng cuối vào `vec!` của `migrations()`:

```rust
        M::up(include_str!("migrations/0007_job_titles.sql")),
        M::up(include_str!("migrations/0008_queue_scheduling.sql")),
    ])
```

- [ ] **Step 5: Thêm trường vào model**

Trong `src-tauri/src/models.rs`, thêm vào cuối `struct DownloadJob` (ngay sau `playlist_title`):

```rust
    /// Thứ tự chạy trong hàng đợi chờ, dùng fractional indexing: số nhỏ chạy
    /// trước, và chèn vào giữa hai mục chỉ cần lấy điểm giữa của chúng nên mỗi
    /// lần kéo-thả chỉ ghi đúng một dòng. `created_at` vẫn là tiêu chí phân
    /// định khi hai giá trị bằng nhau.
    pub queue_position: f64,
    /// Số lần đã tự thử lại vì lỗi tạm thời. Không tính lần chạy đầu tiên.
    pub retry_count: i64,
    /// Khi khác `None` và ở tương lai, job này đang chờ tới lượt thử lại và
    /// bộ điều phối sẽ bỏ qua nó cho tới thời điểm đó (FR-121).
    pub next_retry_at: Option<String>,
```

- [ ] **Step 6: Ghi và đọc trường mới**

Trong `insert_job` (`db/mod.rs:67-99`), đổi câu lệnh INSERT — thêm 3 cột và 3 tham số:

```rust
        conn.execute(
            "INSERT INTO download_jobs (
                id, source_url, platform, media_type, audio_quality, video_quality,
                gallery_mode, selected_gallery_urls, status, progress_percent,
                speed_bytes_per_sec, eta_seconds, error_message, output_directory,
                output_file_path, is_playlist_item, parent_playlist_id,
                retried_from_job_id, created_at, updated_at, title, playlist_title,
                queue_position, retry_count, next_retry_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
            params![
                job.id,
                job.source_url,
                job.platform,
                media_type_str(&job.media_type),
                job.audio_quality,
                job.video_quality,
                job.gallery_mode.as_ref().map(gallery_mode_str),
                selected_gallery_indices_json,
                job.status.as_str(),
                job.progress_percent,
                job.speed_bytes_per_sec,
                job.eta_seconds,
                job.error_message,
                job.output_directory,
                job.output_file_path,
                job.is_playlist_item as i64,
                job.parent_playlist_id,
                job.retried_from_job_id,
                job.created_at,
                job.updated_at,
                job.title,
                job.playlist_title,
                job.queue_position,
                job.retry_count,
                job.next_retry_at,
            ],
        )?;
```

Trong `row_to_job` (`db/mod.rs:280-320`), thêm 3 dòng cuối trước dấu `})`:

```rust
        playlist_title: row.get("playlist_title")?,
        queue_position: row.get("queue_position")?,
        retry_count: row.get("retry_count")?,
        next_retry_at: row.get("next_retry_at")?,
    })
```

- [ ] **Step 7: Sửa mọi nơi khởi tạo DownloadJob**

Trình biên dịch sẽ chỉ ra các vị trí thiếu trường. Có 3 nơi — thêm vào mỗi nơi:

```rust
            queue_position: 0.0,
            retry_count: 0,
            next_retry_at: None,
```

Vị trí: `src-tauri/src/commands/download.rs` (nơi dựng job cho tải đơn, cho fan-out playlist, và cho `create_playlist_download_jobs`) và `src-tauri/src/downloader/queue.rs` (trong `retry`, nơi tạo job mới từ job cũ).

Run: `cd src-tauri && cargo build 2>&1 | grep "missing field"` để liệt kê chính xác các vị trí còn thiếu.

- [ ] **Step 8: Chạy test để xác nhận nó đạt**

Run: `cd src-tauri && cargo test round_trips_scheduling_fields`
Expected: PASS

- [ ] **Step 9: Chạy toàn bộ test Rust để chắc chắn không hỏng gì**

Run: `cd src-tauri && cargo test`
Expected: mọi test đạt (41 test cũ + 1 test mới).

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/db src-tauri/src/models.rs src-tauri/src/commands/download.rs src-tauri/src/downloader/queue.rs
git commit -m "feat(db): add queue_position, retry_count, next_retry_at to download jobs"
```

---

### Task 2: Truy vấn điều phối trong tầng DB

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (thêm phương thức mới sau `list_jobs_by_statuses`, `:173`)
- Test: `src-tauri/src/db/mod.rs` (`mod tests` đã tạo ở Task 1)

- [ ] **Step 1: Viết test thất bại**

Thêm vào `mod tests` trong `src-tauri/src/db/mod.rs`:

```rust
    #[test]
    fn next_dispatchable_job_respects_position_then_created_at() {
        let db = temp_db();
        let mut later = sample_job("later");
        later.queue_position = 5.0;
        later.created_at = "2026-07-26T00:00:00Z".to_string();
        let mut earlier = sample_job("earlier");
        earlier.queue_position = 1.0;
        earlier.created_at = "2026-07-26T23:00:00Z".to_string();
        db.insert_job(&later).unwrap();
        db.insert_job(&earlier).unwrap();

        let picked = db
            .next_dispatchable_job("2026-07-27T00:00:00Z")
            .unwrap()
            .expect("a job is dispatchable");
        assert_eq!(picked.id, "earlier", "queue_position thắng created_at");
    }

    #[test]
    fn next_dispatchable_job_skips_jobs_waiting_to_retry() {
        let db = temp_db();
        let mut waiting = sample_job("waiting");
        waiting.next_retry_at = Some("2026-07-26T00:10:00Z".to_string());
        db.insert_job(&waiting).unwrap();

        let too_early = db.next_dispatchable_job("2026-07-26T00:05:00Z").unwrap();
        assert!(too_early.is_none(), "chưa tới giờ thử lại thì không được chọn");

        let due = db.next_dispatchable_job("2026-07-26T00:10:01Z").unwrap();
        assert_eq!(due.expect("tới giờ rồi").id, "waiting");
    }

    #[test]
    fn next_dispatchable_job_ignores_non_queued_statuses() {
        let db = temp_db();
        let mut paused = sample_job("paused");
        paused.status = JobStatus::Paused;
        db.insert_job(&paused).unwrap();

        assert!(db
            .next_dispatchable_job("2026-07-27T00:00:00Z")
            .unwrap()
            .is_none());
    }

    #[test]
    fn next_queue_position_appends_past_the_maximum() {
        let db = temp_db();
        assert_eq!(db.next_queue_position().unwrap(), 1.0, "hàng đợi rỗng bắt đầu từ 1.0");

        let mut job = sample_job("job-1");
        job.queue_position = 4.0;
        db.insert_job(&job).unwrap();
        assert_eq!(db.next_queue_position().unwrap(), 5.0);
    }

    #[test]
    fn position_between_takes_the_midpoint_of_two_neighbours() {
        assert_eq!(position_between(Some(1.0), Some(2.0)), 1.5);
        assert_eq!(position_between(Some(1.5), Some(2.0)), 1.75);
    }

    #[test]
    fn position_between_handles_the_ends_of_the_list() {
        assert_eq!(position_between(None, None), 1.0, "hàng đợi rỗng");
        assert_eq!(position_between(None, Some(3.0)), 2.0, "thả lên đầu");
        assert_eq!(position_between(Some(3.0), None), 4.0, "thả xuống cuối");
    }

    #[test]
    fn needs_renormalize_only_when_the_gap_has_collapsed() {
        assert!(!needs_renormalize(Some(1.0), Some(2.0)));
        assert!(needs_renormalize(Some(1.0), Some(1.0 + 1e-9)));
        assert!(
            !needs_renormalize(None, Some(1.0)),
            "ở đầu hoặc cuối danh sách thì luôn còn chỗ"
        );
    }

    #[test]
    fn move_job_between_only_rewrites_the_moved_row() {
        let db = temp_db();
        for (id, position) in [("a", 1.0), ("b", 2.0), ("c", 3.0)] {
            let mut job = sample_job(id);
            job.queue_position = position;
            db.insert_job(&job).unwrap();
        }

        // Kéo "c" vào giữa "a" và "b".
        db.move_job_between("c", Some("a"), Some("b")).unwrap();

        assert_eq!(db.get_job("c").unwrap().unwrap().queue_position, 1.5);
        assert_eq!(
            db.get_job("a").unwrap().unwrap().queue_position,
            1.0,
            "hàng xóm không được đụng tới"
        );
        assert_eq!(db.get_job("b").unwrap().unwrap().queue_position, 2.0);
    }

    #[test]
    fn move_job_between_renormalizes_when_the_gap_collapses() {
        let db = temp_db();
        // Hai hàng xóm sát nhau tới mức không còn chỗ chèn vào giữa.
        for (id, position) in [("a", 1.0), ("b", 1.0 + 1e-12), ("c", 9.0)] {
            let mut job = sample_job(id);
            job.queue_position = position;
            db.insert_job(&job).unwrap();
        }

        db.move_job_between("c", Some("a"), Some("b")).unwrap();

        let a = db.get_job("a").unwrap().unwrap().queue_position;
        let b = db.get_job("b").unwrap().unwrap().queue_position;
        let c = db.get_job("c").unwrap().unwrap().queue_position;
        assert!(a < c && c < b, "thứ tự a < c < b phải đúng sau khi chuẩn hoá");
        assert!(b - a > 0.1, "sau chuẩn hoá khe hở phải rộng trở lại");
    }

    #[test]
    fn reset_interrupted_jobs_pauses_downloading_and_fetching() {
        let db = temp_db();
        let mut downloading = sample_job("downloading");
        downloading.status = JobStatus::Downloading;
        let mut fetching = sample_job("fetching");
        fetching.status = JobStatus::FetchingMetadata;
        let mut completed = sample_job("completed");
        completed.status = JobStatus::Completed;
        db.insert_job(&downloading).unwrap();
        db.insert_job(&fetching).unwrap();
        db.insert_job(&completed).unwrap();

        let count = db.reset_interrupted_jobs().unwrap();

        assert_eq!(count, 2);
        assert_eq!(db.get_job("downloading").unwrap().unwrap().status, JobStatus::Paused);
        assert_eq!(db.get_job("fetching").unwrap().unwrap().status, JobStatus::Paused);
        assert_eq!(
            db.get_job("completed").unwrap().unwrap().status,
            JobStatus::Completed,
            "job đã xong không được đụng tới"
        );
    }

    #[test]
    fn mark_job_for_retry_requeues_with_a_future_deadline() {
        let db = temp_db();
        let mut running = sample_job("job-1");
        running.status = JobStatus::Downloading;
        db.insert_job(&running).unwrap();

        db.mark_job_for_retry("job-1", "2026-07-26T00:00:30Z", "network timeout")
            .unwrap();

        let loaded = db.get_job("job-1").unwrap().unwrap();
        assert_eq!(loaded.status, JobStatus::Queued);
        assert_eq!(loaded.retry_count, 1);
        assert_eq!(loaded.next_retry_at.as_deref(), Some("2026-07-26T00:00:30Z"));
        assert_eq!(loaded.error_message.as_deref(), Some("network timeout"));
    }

    #[test]
    fn bulk_update_status_returns_the_ids_it_changed() {
        let db = temp_db();
        let mut queued = sample_job("queued");
        queued.status = JobStatus::Queued;
        let mut done = sample_job("done");
        done.status = JobStatus::Completed;
        db.insert_job(&queued).unwrap();
        db.insert_job(&done).unwrap();

        let changed = db
            .bulk_update_status(&[JobStatus::Queued], JobStatus::Paused)
            .unwrap();

        assert_eq!(changed, vec!["queued".to_string()]);
        assert_eq!(db.get_job("queued").unwrap().unwrap().status, JobStatus::Paused);
    }
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `cd src-tauri && cargo test --lib db::tests`
Expected: FAIL — `no method named next_dispatchable_job found`.

- [ ] **Step 3: Hiện thực các truy vấn**

Trong `src-tauri/src/db/mod.rs`, thêm ngay sau `list_jobs_by_statuses` (`:173`):

```rust
    /// Job kế tiếp mà bộ điều phối được phép khởi chạy: đang `queued`, và
    /// không nằm trong khoảng chờ thử lại. `now_rfc3339` được truyền vào thay
    /// vì đọc đồng hồ ở đây để test kiểm soát được thời gian.
    ///
    /// Thứ tự: `queue_position` trước, `created_at` sau. Vế thứ hai giữ cho
    /// các job cũ (đều mang `queue_position = 0` từ migration 0008) vẫn chạy
    /// đúng thứ tự chúng được tạo.
    pub fn next_dispatchable_job(
        &self,
        now_rfc3339: &str,
    ) -> Result<Option<DownloadJob>, AppError> {
        let conn = self.conn();
        conn.query_row(
            "SELECT * FROM download_jobs
             WHERE status = 'queued'
               AND (next_retry_at IS NULL OR next_retry_at <= ?1)
             ORDER BY queue_position ASC, created_at ASC
             LIMIT 1",
            params![now_rfc3339],
            row_to_job,
        )
        .optional()
        .map_err(AppError::from)
    }

    /// Vị trí cho job mới thêm vào cuối hàng đợi.
    pub fn next_queue_position(&self) -> Result<f64, AppError> {
        let conn = self.conn();
        let max: Option<f64> = conn.query_row(
            "SELECT MAX(queue_position) FROM download_jobs
             WHERE status IN ('queued','paused','downloading','fetching_metadata')",
            [],
            |row| row.get(0),
        )?;
        Ok(position_between(max, None))
    }

    /// Đặt một job vào giữa hai hàng xóm (`None` nghĩa là đầu hoặc cuối danh
    /// sách) — thao tác đằng sau một lần kéo-thả (FR-117).
    ///
    /// Chỉ ghi đúng một dòng. Đó không chỉ là chuyện nhanh: nếu phải đánh số
    /// lại cả danh sách thì một job được thêm vào trong lúc người dùng đang kéo
    /// sẽ bị ghi đè vị trí, vì danh sách giao diện gửi lên đã cũ.
    ///
    /// Giao diện gửi id của hai hàng xóm chứ không gửi số: giao diện không nên
    /// phải biết gì về cách đánh số nội bộ.
    pub fn move_job_between(
        &self,
        job_id: &str,
        before_job_id: Option<&str>,
        after_job_id: Option<&str>,
    ) -> Result<(), AppError> {
        let mut before = self.position_of(before_job_id)?;
        let mut after = self.position_of(after_job_id)?;

        // Chèn liên tiếp vào cùng một chỗ chia đôi khe hở mỗi lần. Khi nó hẹp
        // tới mức f64 sắp hết chỗ, đánh số lại rồi đọc lại hàng xóm.
        if needs_renormalize(before, after) {
            self.renormalize_queue_positions()?;
            before = self.position_of(before_job_id)?;
            after = self.position_of(after_job_id)?;
        }

        let conn = self.conn();
        conn.execute(
            "UPDATE download_jobs SET queue_position = ?1, updated_at = ?2 WHERE id = ?3",
            params![position_between(before, after), Utc::now().to_rfc3339(), job_id],
        )?;
        Ok(())
    }

    fn position_of(&self, job_id: Option<&str>) -> Result<Option<f64>, AppError> {
        let Some(job_id) = job_id else {
            return Ok(None);
        };
        let conn = self.conn();
        conn.query_row(
            "SELECT queue_position FROM download_jobs WHERE id = ?1",
            params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(AppError::from)
    }

    /// Đánh số lại các job chưa kết thúc thành 1.0, 2.0, 3.0… giữ nguyên thứ tự
    /// hiện tại. Chỉ chạy khi khe hở đã hẹp tới ngưỡng — trong sử dụng bình
    /// thường gần như không bao giờ xảy ra.
    pub fn renormalize_queue_positions(&self) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;

        let ids: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM download_jobs
                 WHERE status IN ('queued','paused','downloading','fetching_metadata')
                 ORDER BY queue_position ASC, created_at ASC",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()?
        };

        for (index, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE download_jobs SET queue_position = ?1 WHERE id = ?2",
                params![(index + 1) as f64, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Gọi một lần lúc khởi động: job còn ghi `downloading`/`fetching_metadata`
    /// là tàn dư của một phiên bị đóng đột ngột — tiến trình tải của chúng đã
    /// chết cùng ứng dụng. Chuyển về `paused` để người dùng tiếp tục hoặc huỷ
    /// (FR-115). Trả về số dòng đã đổi.
    pub fn reset_interrupted_jobs(&self) -> Result<usize, AppError> {
        let conn = self.conn();
        let changed = conn.execute(
            "UPDATE download_jobs SET status = 'paused', updated_at = ?1
             WHERE status IN ('downloading','fetching_metadata')",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(changed)
    }

    /// Đưa job về hàng chờ kèm mốc thời gian được phép thử lại (FR-121).
    /// `error_message` được giữ lại để giao diện hiển thị lý do đang chờ.
    pub fn mark_job_for_retry(
        &self,
        job_id: &str,
        next_retry_at_rfc3339: &str,
        error_message: &str,
    ) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE download_jobs
             SET status = 'queued',
                 retry_count = retry_count + 1,
                 next_retry_at = ?1,
                 error_message = ?2,
                 updated_at = ?3
             WHERE id = ?4",
            params![
                next_retry_at_rfc3339,
                error_message,
                Utc::now().to_rfc3339(),
                job_id
            ],
        )?;
        Ok(())
    }

    /// Đổi trạng thái hàng loạt, trả về id các job thực sự bị đổi để tầng gọi
    /// biết cần phát sự kiện cho những job nào (FR-118).
    pub fn bulk_update_status(
        &self,
        from_statuses: &[JobStatus],
        to_status: JobStatus,
    ) -> Result<Vec<String>, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let placeholders = from_statuses.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let status_strs: Vec<&str> = from_statuses.iter().map(|s| s.as_str()).collect();

        let ids: Vec<String> = {
            let sql = format!("SELECT id FROM download_jobs WHERE status IN ({placeholders})");
            let mut stmt = tx.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(status_strs.iter()), |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<rusqlite::Result<Vec<String>>>()?
        };

        for id in &ids {
            tx.execute(
                "UPDATE download_jobs SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![to_status.as_str(), Utc::now().to_rfc3339(), id],
            )?;
        }
        tx.commit()?;
        Ok(ids)
    }
```

`Db` bọc `Mutex<Connection>` và `conn()` trả `MutexGuard`; `transaction()` cần `&mut Connection` nên hai hàm dùng transaction khai báo `let mut conn = self.conn();`.

Thêm hai hàm tự do ở cấp module trong cùng file (ngoài `impl Db`) — chúng thuần tuý tính toán nên kiểm thử được mà không cần cơ sở dữ liệu:

```rust
> **Không có sentinel "chưa xếp chỗ".** Một bản nháp trước của plan này có hàm
> `repair_unpositioned_jobs` coi `queue_position = 0` là "job chưa từng được
> xếp chỗ" và dồn các dòng đó xuống cuối lúc khởi động. Nó tạo ra một lỗi thấy
> được: `position_between(None, Some(1.0))` trả về đúng `0.0`, nên **kéo một
> job lên đầu hàng đợi rồi mở lại ứng dụng sẽ thấy nó nằm dưới đáy**.
>
> Vá công thức đầu danh sách không giải quyết được: `after / 2.0` chỉ đẩy lùi
> vấn đề — thả liên tiếp vào đầu sẽ chia đôi dần về 0 và cuối cùng chạm đúng
> giá trị đó, mà `needs_renormalize` không bắt được vì không có hàng xóm phía
> trước để đo khe hở.
>
> Cách đúng là bỏ hẳn sentinel. Vị trí âm và bằng 0 đều hợp lệ — thứ tự chỉ
> quan tâm giá trị tương đối. Bất biến "mọi job đều có vị trí" được bảo đảm ở
> chỗ chèn: `enqueue` (Task 5) luôn gán `next_queue_position()`.

/// Khe hở hẹp nhất còn chấp nhận được giữa hai vị trí liền kề.
///
/// `f64` có 52 bit phần định trị, nên trên lý thuyết còn chia đôi được sâu hơn
/// ngưỡng này rất nhiều. Đặt ngưỡng cao hơn giới hạn thật nhiều bậc để không
/// bao giờ chạm tới vùng mà phép lấy điểm giữa trả về đúng bằng một trong hai
/// đầu mút — lúc đó thứ tự sẽ hỏng một cách âm thầm.
const MIN_POSITION_GAP: f64 = 1e-6;

/// Vị trí nằm giữa hai hàng xóm. `None` nghĩa là không có hàng xóm ở phía đó,
/// tức là đang thả vào đầu hoặc cuối danh sách.
pub fn position_between(before: Option<f64>, after: Option<f64>) -> f64 {
    match (before, after) {
        (None, None) => 1.0,
        (None, Some(after)) => after - 1.0,
        (Some(before), None) => before + 1.0,
        (Some(before), Some(after)) => (before + after) / 2.0,
    }
}

/// Khe hở giữa hai hàng xóm đã hẹp tới mức phải đánh số lại chưa.
///
/// Chỉ đúng khi có cả hai hàng xóm: ở đầu hoặc cuối danh sách thì luôn còn chỗ
/// vì ta cộng/trừ hẳn 1.0 chứ không chia đôi.
pub fn needs_renormalize(before: Option<f64>, after: Option<f64>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => (after - before).abs() < MIN_POSITION_GAP,
        _ => false,
    }
}
```

- [ ] **Step 4: Cho phép so sánh JobStatus trong assert**

`JobStatus` đã có `PartialEq, Eq` (`models.rs:37`) nên `assert_eq!` chạy được. Nếu trình biên dịch báo thiếu `Debug`, kiểm tra lại dòng derive — nó đã có `Debug`.

- [ ] **Step 5: Chạy test để xác nhận nó đạt**

Run: `cd src-tauri && cargo test --lib db::tests`
Expected: PASS — 8 test.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "feat(db): add scheduling queries for the download dispatcher"
```

---

### Task 3: Phân loại lỗi tạm thời và tính khoảng chờ

**Files:**
- Create: `src-tauri/src/downloader/retry.rs`
- Modify: `src-tauri/src/downloader/mod.rs` (khai báo module)
- Modify: `src-tauri/src/downloader/ytdlp.rs:234-251` (`classify_ytdlp_error`)
- Modify: `src-tauri/src/downloader/gallery_dl.rs:338-350` (`classify_gallery_dl_error`)
- Test: trong chính `retry.rs` và các `mod tests` sẵn có

- [ ] **Step 1: Viết test thất bại cho module retry**

Tạo `src-tauri/src/downloader/retry.rs` với phần test trước:

```rust
//! Chính sách thử lại: quyết định lỗi nào đáng thử lại và chờ bao lâu.
//!
//! Tách riêng khỏi `queue` để logic quyết định này kiểm thử được mà không cần
//! dựng cả một hàng đợi, và để cả yt-dlp lẫn gallery-dl dùng chung một chính
//! sách (FR-120, FR-121).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_errors_are_transient() {
        assert!(is_transient("NETWORK_ERROR"));
    }

    #[test]
    fn content_errors_are_permanent() {
        assert!(!is_transient("ACCESS_DENIED"));
        assert!(!is_transient("UNSUPPORTED_PLATFORM"));
        assert!(!is_transient("INVALID_QUALITY_OPTION"));
    }

    #[test]
    fn generic_download_failures_are_permanent() {
        // DOWNLOAD_FAILED là nhóm gom mọi lỗi chưa nhận diện được. Coi nó là
        // vĩnh viễn: thà báo lỗi ngay còn hơn bắt người dùng chờ hết ba vòng
        // thử lại vô ích (SC-106). Lỗi mạng thật đã được tách ra thành
        // NETWORK_ERROR ở tầng phân loại rồi.
        assert!(!is_transient("DOWNLOAD_FAILED"));
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff_seconds(0), 5);
        assert_eq!(backoff_seconds(1), 10);
        assert_eq!(backoff_seconds(2), 20);
        assert_eq!(backoff_seconds(3), 40);
        assert_eq!(backoff_seconds(20), 300, "chặn trên ở 5 phút");
    }

    #[test]
    fn should_retry_stops_at_the_configured_limit() {
        assert!(should_retry("NETWORK_ERROR", 0, 3));
        assert!(should_retry("NETWORK_ERROR", 2, 3));
        assert!(!should_retry("NETWORK_ERROR", 3, 3), "đã dùng hết lượt");
        assert!(!should_retry("ACCESS_DENIED", 0, 3), "lỗi vĩnh viễn không thử lại");
        assert!(!should_retry("NETWORK_ERROR", 0, 0), "người dùng tắt retry");
    }
}
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Trước tiên khai báo module — trong `src-tauri/src/downloader/mod.rs` thêm:

```rust
pub mod retry;
```

Run: `cd src-tauri && cargo test --lib downloader::retry`
Expected: FAIL — `cannot find function is_transient in this scope`.

- [ ] **Step 3: Hiện thực**

Thêm vào đầu `src-tauri/src/downloader/retry.rs`, phía trên `mod tests`:

```rust
/// Khoảng chờ tối đa giữa hai lần thử. Vượt quá mức này thì việc chờ tiếp
/// không còn giúp gì mà chỉ làm người dùng tưởng ứng dụng bị treo.
const MAX_BACKOFF_SECONDS: u64 = 300;

/// Khoảng chờ cho lần thử đầu tiên. Đủ dài để một lần chập mạng kịp hồi phục,
/// đủ ngắn để không gây khó chịu.
const BASE_BACKOFF_SECONDS: u64 = 5;

/// Lỗi có đáng thử lại không, dựa trên mã lỗi ổn định của `AppError`.
///
/// Chỉ mã `NETWORK_ERROR` được coi là tạm thời. Mọi mã khác — kể cả
/// `DOWNLOAD_FAILED` vốn là nhóm gom — đều bị coi là vĩnh viễn, vì thử lại một
/// lỗi vĩnh viễn chỉ làm chậm phản hồi mà không đổi được kết quả.
pub fn is_transient(error_code: &str) -> bool {
    error_code == "NETWORK_ERROR"
}

/// Số giây chờ trước lần thử thứ `retry_count + 1`. Tăng gấp đôi mỗi lần,
/// chặn trên ở `MAX_BACKOFF_SECONDS`.
pub fn backoff_seconds(retry_count: i64) -> u64 {
    let exponent = retry_count.clamp(0, 16) as u32;
    BASE_BACKOFF_SECONDS
        .saturating_mul(2u64.saturating_pow(exponent))
        .min(MAX_BACKOFF_SECONDS)
}

/// Có nên tự thử lại không: lỗi phải là tạm thời và chưa dùng hết số lượt.
pub fn should_retry(error_code: &str, retry_count: i64, max_retries: i64) -> bool {
    is_transient(error_code) && retry_count < max_retries
}
```

- [ ] **Step 4: Chạy test để xác nhận nó đạt**

Run: `cd src-tauri && cargo test --lib downloader::retry`
Expected: PASS — 5 test.

- [ ] **Step 5: Viết test thất bại cho việc nhận diện lỗi mạng**

Thêm vào `mod tests` cuối `src-tauri/src/downloader/ytdlp.rs`:

```rust
    #[test]
    fn classifies_network_failures_separately() {
        for stderr in [
            "ERROR: network timeout",
            "ERROR: [Errno 110] Connection timed out",
            "ERROR: unable to download video data: <urlopen error [Errno -3] Temporary failure in name resolution>",
            "ERROR: Unable to download webpage: HTTP Error 503: Service Unavailable",
            "ERROR: HTTP Error 429: Too Many Requests",
            "ERROR: Connection reset by peer",
        ] {
            assert_eq!(
                classify_ytdlp_error(stderr).code,
                "NETWORK_ERROR",
                "phải nhận ra là lỗi mạng: {stderr}"
            );
        }
    }

    #[test]
    fn content_failures_do_not_become_network_errors() {
        assert_eq!(
            classify_ytdlp_error("ERROR: Private video. Sign in if you've been granted access").code,
            "ACCESS_DENIED"
        );
        assert_eq!(
            classify_ytdlp_error("ERROR: Unsupported URL: https://example.com").code,
            "UNSUPPORTED_PLATFORM"
        );
        assert_eq!(
            classify_ytdlp_error("ERROR: something unusual happened").code,
            "DOWNLOAD_FAILED"
        );
    }
```

- [ ] **Step 6: Chạy test để xác nhận nó thất bại**

Run: `cd src-tauri && cargo test --lib downloader::ytdlp`
Expected: FAIL — `assertion left == right failed: left: "DOWNLOAD_FAILED", right: "NETWORK_ERROR"`.

Lưu ý: test cũ khẳng định `"ERROR: network timeout"` phải ra `DOWNLOAD_FAILED` (`ytdlp.rs:310-313`) — test đó **phải được sửa lại**, vì hành vi cũ chính là thứ FR-120 yêu cầu thay đổi. Xoá hoặc cập nhật nó thành mong đợi mới.

- [ ] **Step 7: Hiện thực phân loại lỗi mạng**

Trong `src-tauri/src/downloader/ytdlp.rs`, sửa `classify_ytdlp_error` — thêm nhánh kiểm tra lỗi mạng **trước** nhánh `DOWNLOAD_FAILED` gom cuối.

> **Chỉ chèn thêm nhánh mới, không viết lại cả hàm.** Đoạn mã minh hoạ dưới đây từng có một lỗi: nó thay cách trích thông báo từ `stderr.lines().last().unwrap_or(stderr)` thành `stderr.trim()` ở mọi nhánh, khiến toàn bộ stderr nhiều dòng của yt-dlp bị đẩy vào thông báo hiển thị cho người dùng thay vì chỉ dòng `ERROR:` cuối cùng. Giữ nguyên cách trích thông báo hiện có ở mọi nhánh, kể cả nhánh mới.

```rust
/// Các dấu hiệu cho thấy lỗi đến từ đường truyền chứ không từ nội dung. Thử
/// lại chỉ có ý nghĩa với nhóm này (FR-120).
const NETWORK_ERROR_MARKERS: [&str; 12] = [
    "network",
    "timed out",
    "timeout",
    "connection reset",
    "connection refused",
    "connection aborted",
    "temporary failure",
    "name resolution",
    "unable to connect",
    "http error 429",
    "http error 502",
    "http error 503",
];

pub fn classify_ytdlp_error(stderr: &str) -> AppError {
    let lower = stderr.to_lowercase();

    if lower.contains("private video")
        || lower.contains("sign in")
        || lower.contains("login")
        || lower.contains("drm")
        || lower.contains("premium")
    {
        return AppError::access_denied(stderr.trim());
    }

    if lower.contains("unsupported url") || lower.contains("no extractor") {
        return AppError::new("UNSUPPORTED_PLATFORM", stderr.trim());
    }

    // Kiểm tra lỗi mạng SAU các lỗi nội dung: một thông báo "private video"
    // đôi khi cũng chứa từ "connection", và lỗi nội dung phải thắng để không
    // bị thử lại vô ích.
    if NETWORK_ERROR_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return AppError::new("NETWORK_ERROR", stderr.trim());
    }

    AppError::new("DOWNLOAD_FAILED", stderr.trim())
}
```

Giữ nguyên thứ tự các nhánh cũ; chỉ chèn khối `NETWORK_ERROR_MARKERS` vào ngay trước `DOWNLOAD_FAILED`.

- [ ] **Step 8: Làm điều tương tự cho gallery-dl**

Trong `src-tauri/src/downloader/gallery_dl.rs`, sửa `classify_gallery_dl_error` — chèn cùng khối kiểm tra ngay trước nhánh gom cuối:

```rust
    if crate::downloader::ytdlp::NETWORK_ERROR_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return AppError::new("NETWORK_ERROR", stderr.trim());
    }
```

Để dùng được từ module khác, đổi khai báo hằng trong `ytdlp.rs` thành `pub const NETWORK_ERROR_MARKERS`.

Thêm test tương ứng vào `mod tests` của `gallery_dl.rs`:

```rust
    #[test]
    fn classifies_network_failures_separately() {
        assert_eq!(
            classify_gallery_dl_error("ConnectionError: Connection timed out").code,
            "NETWORK_ERROR"
        );
        assert_eq!(
            classify_gallery_dl_error("HttpError: 403 Forbidden").code,
            "ACCESS_DENIED"
        );
    }
```

- [ ] **Step 9: Cập nhật test cũ đang khẳng định hành vi ngược lại**

Sửa test ở `ytdlp.rs:309-313` và `gallery_dl.rs:427-430` (cả hai đang khẳng định network timeout ra `DOWNLOAD_FAILED`) để phản ánh hành vi mới, hoặc xoá vì đã bị các test mới ở Step 5/8 bao phủ.

- [ ] **Step 10: Chạy toàn bộ test Rust**

Run: `cd src-tauri && cargo test`
Expected: PASS toàn bộ.

- [ ] **Step 11: Thêm chuỗi dịch cho mã lỗi mới**

Trong `src/locales/en.json`, mục `errors`, thêm:

```json
    "NETWORK_ERROR": "Network problem. Check your connection — the download will retry automatically."
```

Trong `src/locales/vi.json`, mục `errors`, thêm:

```json
    "NETWORK_ERROR": "Sự cố mạng. Kiểm tra kết nối — bản tải sẽ tự thử lại."
```

- [ ] **Step 12: Commit**

```bash
git add src-tauri/src/downloader src/locales
git commit -m "feat(downloader): classify network failures as transient and add retry policy"
```

---

### Task 4: Bốn setting mới

**Files:**
- Modify: `src-tauri/src/models.rs:221-229` (`AppSettings`)
- Modify: `src-tauri/src/db/mod.rs:243-260` (`get_settings`, `update_settings`)
- Modify: `src-tauri/src/commands/settings.rs:14-42`
- Modify: `src/types/settings.ts`
- Test: `src-tauri/src/db/mod.rs` (`mod tests`)

- [ ] **Step 1: Viết test thất bại**

Thêm vào `mod tests` trong `src-tauri/src/db/mod.rs`:

```rust
    #[test]
    fn new_settings_have_sensible_defaults() {
        let db = temp_db();
        let settings = db.get_settings().unwrap();

        assert_eq!(settings.max_concurrent_downloads, 3, "giữ nguyên hành vi cũ");
        assert_eq!(settings.rate_limit_kbps, 0, "0 nghĩa là không giới hạn");
        assert_eq!(settings.max_retry_attempts, 3);
        assert!(!settings.run_in_background, "chạy nền phải mặc định tắt");
    }

    #[test]
    fn settings_round_trip_through_the_database() {
        let db = temp_db();
        let mut settings = db.get_settings().unwrap();
        settings.max_concurrent_downloads = 6;
        settings.rate_limit_kbps = 2048;
        settings.max_retry_attempts = 0;
        settings.run_in_background = true;
        db.update_settings(&settings).unwrap();

        let reloaded = db.get_settings().unwrap();
        assert_eq!(reloaded.max_concurrent_downloads, 6);
        assert_eq!(reloaded.rate_limit_kbps, 2048);
        assert_eq!(reloaded.max_retry_attempts, 0);
        assert!(reloaded.run_in_background);
    }
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `cd src-tauri && cargo test --lib db::tests::new_settings`
Expected: FAIL — `no field max_concurrent_downloads on type AppSettings`.

- [ ] **Step 3: Mở rộng model**

Trong `src-tauri/src/models.rs`, thêm vào `struct AppSettings`:

```rust
    /// Số tác vụ được chạy đồng thời (FR-112). Bộ điều phối đọc lại giá trị
    /// này mỗi vòng nên đổi lúc đang chạy có hiệu lực ngay.
    pub max_concurrent_downloads: u32,
    /// Giới hạn tốc độ cho **mỗi** tiến trình tải, tính bằng KB/s. 0 = không
    /// giới hạn. Là giới hạn theo tiến trình chứ không phải tổng băng thông —
    /// giao diện phải nói rõ điều này (xem phần Assumptions của spec).
    pub rate_limit_kbps: u32,
    /// Số lần tự thử lại tối đa cho lỗi tạm thời. 0 = tắt hẳn tự thử lại.
    pub max_retry_attempts: u32,
    /// Đóng cửa sổ thì thu về khay hệ thống thay vì thoát (FR-127).
    pub run_in_background: bool,
```

- [ ] **Step 4: Đọc và ghi trong tầng DB**

Trong `src-tauri/src/db/mod.rs`, `get_settings` — thêm 4 dòng vào struct trả về:

```rust
            max_concurrent_downloads: Self::get_setting_or_default(&conn, "max_concurrent_downloads", "3")?
                .parse()
                .unwrap_or(3),
            rate_limit_kbps: Self::get_setting_or_default(&conn, "rate_limit_kbps", "0")?
                .parse()
                .unwrap_or(0),
            max_retry_attempts: Self::get_setting_or_default(&conn, "max_retry_attempts", "3")?
                .parse()
                .unwrap_or(3),
            run_in_background: Self::get_setting_or_default(&conn, "run_in_background", "0")? == "1",
```

`unwrap_or` ở đây là chủ ý: một giá trị rác trong DB (do người dùng sửa tay hoặc lỗi ghi) phải rơi về mặc định chứ không được làm hỏng cả màn hình cài đặt.

Trong `update_settings` — thêm 4 dòng:

```rust
        Self::set_setting(&conn, "max_concurrent_downloads", &settings.max_concurrent_downloads.to_string())?;
        Self::set_setting(&conn, "rate_limit_kbps", &settings.rate_limit_kbps.to_string())?;
        Self::set_setting(&conn, "max_retry_attempts", &settings.max_retry_attempts.to_string())?;
        Self::set_setting(&conn, "run_in_background", if settings.run_in_background { "1" } else { "0" })?;
```

- [ ] **Step 5: Mở rộng lệnh cập nhật cài đặt**

Trong `src-tauri/src/commands/settings.rs`, thêm vào `UpdateSettingsInput`:

```rust
    pub max_concurrent_downloads: Option<u32>,
    pub rate_limit_kbps: Option<u32>,
    pub max_retry_attempts: Option<u32>,
    pub run_in_background: Option<bool>,
```

Và trong `update_settings`, trước `db.update_settings(&current)?`:

```rust
    if let Some(value) = patch.max_concurrent_downloads {
        // Chặn trên/dưới ở đây chứ không chỉ ở giao diện: lệnh này gọi được
        // trực tiếp, và giá trị 0 sẽ làm bộ điều phối không bao giờ chạy job.
        current.max_concurrent_downloads = value.clamp(1, 8);
    }
    if let Some(value) = patch.rate_limit_kbps {
        current.rate_limit_kbps = value;
    }
    if let Some(value) = patch.max_retry_attempts {
        current.max_retry_attempts = value.min(10);
    }
    if let Some(value) = patch.run_in_background {
        current.run_in_background = value;
    }
```

- [ ] **Step 6: Đồng bộ kiểu ở frontend**

Trong `src/types/settings.ts`, thêm vào `interface AppSettings`:

```typescript
  max_concurrent_downloads: number;
  rate_limit_kbps: number;
  max_retry_attempts: number;
  run_in_background: boolean;
```

- [ ] **Step 7: Chạy test**

Run: `cd src-tauri && cargo test --lib db::tests`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/db/mod.rs src-tauri/src/commands/settings.rs src/types/settings.ts
git commit -m "feat(settings): add concurrency, rate limit, retry and background settings"
```

---

### Task 5: Bộ điều phối thay cho semaphore

Đây là task lõi của cả phase. Đọc kỹ `src-tauri/src/downloader/queue.rs:52-190` trước khi bắt đầu.

**Files:**
- Create: `src-tauri/src/downloader/scheduler.rs`
- Modify: `src-tauri/src/downloader/mod.rs`
- Modify: `src-tauri/src/downloader/queue.rs:17` (bỏ hằng), `:56-190` (struct + enqueue + spawn_run + pause/resume/cancel)
- Test: `src-tauri/src/downloader/scheduler.rs`

- [ ] **Step 1: Viết test thất bại cho logic quyết định của bộ điều phối**

Tạo `src-tauri/src/downloader/scheduler.rs`:

```rust
//! Bộ điều phối hàng đợi tải.
//!
//! Thay cho cơ chế cũ (`enqueue` spawn task ngay rồi task đó chờ `Semaphore`),
//! mô hình ở đây tách hẳn "xếp hàng" khỏi "chạy": `enqueue` chỉ ghi DB, còn
//! một task dispatcher duy nhất quyết định khi nào job nào được chạy.
//!
//! Ba thứ mà cơ chế cũ không làm được và mô hình này làm được:
//! - **Sắp xếp lại thứ tự**: thứ tự nằm ở cột `queue_position` trong DB, không
//!   phải ở thứ tự các task đã spawn xếp hàng trước semaphore.
//! - **Đổi số luồng lúc đang chạy**: `max_concurrent` là `AtomicUsize` được
//!   đọc lại mỗi vòng, thay vì số permit cố định lúc khởi tạo semaphore.
//! - **Chờ thử lại có thể huỷ**: job chờ retry là một dòng DB ở trạng thái
//!   `queued` với `next_retry_at` ở tương lai, không phải một task đang ngủ.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Nhịp tick của dispatcher. Cần một nhịp cố định (chứ không chỉ dựa vào tín
/// hiệu đánh thức) vì job chờ thử lại đến hạn theo đồng hồ, không có ai đánh
/// thức hộ.
pub const TICK_INTERVAL_MS: u64 = 1000;

/// Số slot còn trống để khởi chạy job mới.
///
/// Trả 0 khi số đang chạy đã bằng hoặc vượt giới hạn — trường hợp "vượt" xảy
/// ra hợp lệ khi người dùng giảm số luồng lúc đang chạy: các job đang chạy
/// được chạy nốt (FR-113), chỉ không có job mới nào được khởi chạy thêm.
pub fn available_slots(running_count: usize, max_concurrent: &AtomicUsize) -> usize {
    let max = max_concurrent.load(Ordering::Relaxed);
    max.saturating_sub(running_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_free_slots_when_below_the_limit() {
        let max = AtomicUsize::new(3);
        assert_eq!(available_slots(0, &max), 3);
        assert_eq!(available_slots(2, &max), 1);
    }

    #[test]
    fn reports_no_slots_when_at_the_limit() {
        let max = AtomicUsize::new(3);
        assert_eq!(available_slots(3, &max), 0);
    }

    #[test]
    fn reports_no_slots_when_the_limit_was_lowered_mid_flight() {
        // Người dùng hạ từ 5 xuống 2 trong khi 4 job đang chạy: không được trả
        // về số âm và cũng không được khởi chạy thêm gì (FR-113).
        let max = AtomicUsize::new(2);
        assert_eq!(available_slots(4, &max), 0);
    }

    #[test]
    fn picks_up_a_raised_limit_without_being_recreated() {
        let max = AtomicUsize::new(1);
        assert_eq!(available_slots(1, &max), 0);
        max.store(4, Ordering::Relaxed);
        assert_eq!(available_slots(1, &max), 3, "đổi số luồng có hiệu lực ngay");
    }
}
```

- [ ] **Step 2: Khai báo module và chạy test**

Trong `src-tauri/src/downloader/mod.rs` thêm:

```rust
pub mod scheduler;
```

Run: `cd src-tauri && cargo test --lib downloader::scheduler`
Expected: PASS — 4 test (module này thuần logic nên đạt ngay; nó là phần kiểm chứng được của bộ điều phối).

- [ ] **Step 3: Commit phần thuần logic**

```bash
git add src-tauri/src/downloader/scheduler.rs src-tauri/src/downloader/mod.rs
git commit -m "feat(downloader): add scheduler slot accounting"
```

- [ ] **Step 4: Thay struct DownloadQueue**

Trong `src-tauri/src/downloader/queue.rs`, **xoá** hằng ở dòng 17:

```rust
const MAX_CONCURRENT_DOWNLOADS: usize = 3;
```

Đổi phần khai báo struct và `new` (`queue.rs:56-74`) thành:

```rust
pub struct DownloadQueue {
    db: Arc<Db>,
    app: AppHandle,
    running: Arc<AsyncMutex<HashMap<String, RunningJob>>>,
    /// Đọc lại mỗi vòng dispatch nên người dùng đổi số luồng là có hiệu lực
    /// ngay, không cần dựng lại hàng đợi (FR-113).
    max_concurrent: Arc<AtomicUsize>,
    /// Đánh thức dispatcher khi có việc mới, để không phải đợi hết nhịp tick.
    wake: Arc<Notify>,
}

/// Một lần chạy cụ thể của một job.
///
/// `run_id` tồn tại để sửa lỗi tranh chấp: khi người dùng tạm dừng rồi tiếp
/// tục rất nhanh, task của lần chạy cũ có thể kết thúc *sau* khi lần chạy mới
/// đã đăng ký, và nếu nó xoá theo `job_id` thì sẽ xoá nhầm handle của lần chạy
/// mới, khiến job đó không còn tạm dừng hay huỷ được nữa. Task chỉ được xoá
/// entry nếu `run_id` khớp với chính lần chạy của nó (FR-125).
struct RunningJob {
    cancel_tx: watch::Sender<bool>,
    run_id: u64,
}

impl DownloadQueue {
    pub fn new(db: Arc<Db>, app: AppHandle, max_concurrent: usize) -> Self {
        let queue = Self {
            db,
            app,
            running: Arc::new(AsyncMutex::new(HashMap::new())),
            max_concurrent: Arc::new(AtomicUsize::new(max_concurrent.clamp(1, 8))),
            wake: Arc::new(Notify::new()),
        };
        queue.spawn_dispatcher();
        queue
    }

    /// Người dùng đổi số luồng trong Cài đặt. Đánh thức dispatcher ngay để
    /// việc tăng số luồng có hiệu lực tức thì thay vì đợi hết nhịp tick.
    pub fn set_max_concurrent(&self, value: usize) {
        self.max_concurrent
            .store(value.clamp(1, 8), Ordering::Relaxed);
        self.wake.notify_one();
    }

    fn handles(&self) -> QueueHandles {
        QueueHandles {
            db: Arc::clone(&self.db),
            app: self.app.clone(),
            running: Arc::clone(&self.running),
            max_concurrent: Arc::clone(&self.max_concurrent),
            wake: Arc::clone(&self.wake),
        }
    }
}

/// Bản sao các handle dùng chung, để task nền không phải giữ `&DownloadQueue`.
#[derive(Clone)]
struct QueueHandles {
    db: Arc<Db>,
    app: AppHandle,
    running: Arc<AsyncMutex<HashMap<String, RunningJob>>>,
    max_concurrent: Arc<AtomicUsize>,
    wake: Arc<Notify>,
}
```

Thêm các import cần thiết vào đầu file:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;
use crate::downloader::scheduler::{available_slots, TICK_INTERVAL_MS};
```

và **bỏ** `use tokio::sync::Semaphore;`.

- [ ] **Step 5: Viết vòng lặp dispatcher**

Thêm vào `impl DownloadQueue` trong `queue.rs`:

```rust
    /// Task duy nhất quyết định job nào được chạy. Thức dậy theo nhịp tick
    /// hoặc khi được đánh thức, rồi khởi chạy tối đa số job mà slot cho phép.
    ///
    /// Nhịp tick là bắt buộc chứ không thừa: job đang chờ thử lại đến hạn theo
    /// đồng hồ và không có ai gọi `wake` hộ nó.
    fn spawn_dispatcher(&self) {
        let handles = self.handles();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = handles.wake.notified() => {}
                    _ = tokio::time::sleep(std::time::Duration::from_millis(TICK_INTERVAL_MS)) => {}
                }
                if let Err(err) = dispatch_ready(&handles).await {
                    crate::logging::log_warn(
                        &handles.app,
                        &format!("dispatcher tick failed: {err}"),
                    );
                }
            }
        });
    }
```

Và thêm hàm tự do trong cùng file:

```rust
/// Khởi chạy job cho tới khi hết slot hoặc hết job đủ điều kiện.
///
/// Chạy tuần tự trong đúng một task nên không cần khoá gì thêm: giữa lúc chọn
/// job và lúc đánh dấu nó `downloading` không có ai khác xen vào chọn trùng.
async fn dispatch_ready(handles: &QueueHandles) -> Result<(), AppError> {
    loop {
        let running_count = handles.running.lock().await.len();
        if available_slots(running_count, &handles.max_concurrent) == 0 {
            return Ok(());
        }

        let now = Utc::now().to_rfc3339();
        let Some(job) = handles.db.next_dispatchable_job(&now)? else {
            return Ok(());
        };

        start_job(handles, job).await?;
    }
}

/// Chuyển một job từ hàng chờ sang đang chạy: đánh dấu trạng thái, đăng ký
/// handle huỷ, rồi spawn task thực thi.
async fn start_job(handles: &QueueHandles, job: DownloadJob) -> Result<(), AppError> {
    let job_id = job.id.clone();
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let run_id = next_run_id();

    // Đánh dấu `downloading` TRƯỚC khi spawn, vì `next_dispatchable_job` chỉ
    // lọc theo `status = 'queued'` — nếu để sau, vòng lặp dispatch kế tiếp sẽ
    // chọn lại đúng job này.
    handles
        .db
        .update_job_status(&job_id, JobStatus::Downloading, None)?;
    emit_status_changed(&handles.app, &job_id, JobStatus::Downloading, None, None);

    handles
        .running
        .lock()
        .await
        .insert(job_id.clone(), RunningJob { cancel_tx, run_id });

    let task_handles = handles.clone();
    let task_job_id = job_id.clone();
    tokio::spawn(async move {
        let outcome = run_job(&task_handles, job, cancel_rx).await;
        finish_job(&task_handles, &task_job_id, run_id, outcome).await;
        // Slot vừa trống — báo dispatcher biết ngay thay vì đợi hết nhịp tick.
        task_handles.wake.notify_one();
    });

    Ok(())
}

/// Bộ đếm lần chạy, chỉ dùng để phân biệt các lần chạy của cùng một job.
fn next_run_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}
```

Thêm import `use std::sync::atomic::AtomicU64;`.

- [ ] **Step 6: Viết hàm kết thúc job, nơi quyết định thử lại**

Thêm vào `queue.rs`:

```rust
/// Xử lý kết quả một lần chạy: hoàn tất, thất bại vĩnh viễn, hay xếp lại hàng
/// để thử lại.
///
/// Chỉ gỡ handle khỏi `running` khi `run_id` khớp — xem giải thích ở
/// `RunningJob::run_id`.
async fn finish_job(
    handles: &QueueHandles,
    job_id: &str,
    run_id: u64,
    outcome: Result<(), AppError>,
) {
    {
        let mut running = handles.running.lock().await;
        if running.get(job_id).map(|entry| entry.run_id) == Some(run_id) {
            running.remove(job_id);
        }
    }

    let Err(err) = outcome else {
        return; // `run_job` đã tự đánh dấu hoàn tất và phát sự kiện.
    };

    // Người dùng chủ động dừng thì không phải lỗi và tuyệt đối không thử lại:
    // `pause`/`cancel` đã đặt trạng thái cuối cùng rồi.
    if err.code == "CANCELED" {
        return;
    }

    let max_retries = handles
        .db
        .get_settings()
        .map(|s| s.max_retry_attempts as i64)
        .unwrap_or(3);
    let retry_count = handles
        .db
        .get_job(job_id)
        .ok()
        .flatten()
        .map(|job| job.retry_count)
        .unwrap_or(0);

    if crate::downloader::retry::should_retry(&err.code, retry_count, max_retries) {
        let delay = crate::downloader::retry::backoff_seconds(retry_count);
        let next_retry_at = (Utc::now() + chrono::Duration::seconds(delay as i64)).to_rfc3339();
        crate::logging::log_warn(
            &handles.app,
            &format!("job {job_id} failed with {}; retrying in {delay}s", err.code),
        );
        if handles
            .db
            .mark_job_for_retry(job_id, &next_retry_at, &err.message)
            .is_ok()
        {
            emit_status_changed(
                &handles.app,
                job_id,
                JobStatus::Queued,
                Some(&err.message),
                None,
            );
            return;
        }
    }

    let _ = handles
        .db
        .update_job_status(job_id, JobStatus::Failed, Some(&err.message));
    emit_status_changed(
        &handles.app,
        job_id,
        JobStatus::Failed,
        Some(&err.message),
        None,
    );
}
```

- [ ] **Step 7: Rút gọn enqueue**

Thay `enqueue` và **xoá hẳn** `spawn_run` (`queue.rs:76-113`):

```rust
    /// Ghi job vào DB ở cuối hàng đợi rồi đánh thức dispatcher. Không tự chạy
    /// gì cả — việc quyết định khi nào chạy hoàn toàn thuộc về dispatcher.
    pub async fn enqueue(&self, mut job: DownloadJob) -> Result<(), AppError> {
        job.queue_position = self.db.next_queue_position()?;
        job.status = JobStatus::Queued;
        self.db.insert_job(&job)?;
        emit_status_changed(&self.app, &job.id, JobStatus::Queued, None, None);
        self.wake.notify_one();
        Ok(())
    }
```

- [ ] **Step 8: Bỏ vòng lặp thử lại trong run_job**

`run_job` hiện có `for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS` ở `queue.rs:210` và bản sao trong `run_gallery_job` ở `:371-402` và `:461-512`. Thay mỗi vòng lặp bằng **một lần chạy duy nhất** trả `Err` ra ngoài — `finish_job` giờ là nơi duy nhất quyết định có thử lại hay không.

Giữ nguyên hai cơ chế thử lại **nội bộ** vì chúng không phải retry vì lỗi mạng:
- Vòng thử lại khi video tải về thiếu luồng âm thanh (`queue.rs:267-276`).
- Bước phục hồi âm thanh `recover_missing_audio` (`queue.rs:284-296`).

Xoá hằng `MAX_DOWNLOAD_ATTEMPTS` ở `queue.rs:25`.

Đổi chữ ký `run_job` và `run_gallery_job` thành hàm tự do nhận `&QueueHandles`:

```rust
async fn run_job(
    handles: &QueueHandles,
    job: DownloadJob,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<(), AppError>
```

Bên trong, thay `self.db` → `handles.db`, `self.app` → `handles.app`.

- [ ] **Step 9: Sửa pause / resume / cancel cho khớp mô hình mới**

Thay ba hàm ở `queue.rs:115-151`:

```rust
    /// Dừng job đang chạy hoặc đang chờ. `to_status` là trạng thái cuối cùng
    /// (`Paused` hay `Canceled`).
    ///
    /// Gửi tín hiệu huỷ khiến `tokio::select!` trong `run_job` thắng, tiến
    /// trình con bị drop, và `kill_on_drop(true)` giết nó. Với job còn đang
    /// chờ trong DB thì không có gì để giết — chỉ cần đổi trạng thái là
    /// dispatcher sẽ không chọn nó nữa.
    async fn stop_job(&self, job_id: &str, to_status: JobStatus) -> Result<(), AppError> {
        if let Some(entry) = self.running.lock().await.get(job_id) {
            let _ = entry.cancel_tx.send(true);
        }
        // Xoá mốc chờ thử lại: người dùng đã can thiệp thủ công nên vòng thử
        // lại tự động phải dừng hẳn (FR-123).
        self.db.clear_retry_deadline(job_id)?;
        self.db.update_job_status(job_id, to_status.clone(), None)?;
        emit_status_changed(&self.app, job_id, to_status, None, None);
        self.wake.notify_one();
        Ok(())
    }

    pub async fn cancel(&self, job_id: &str) -> Result<(), AppError> {
        self.stop_job(job_id, JobStatus::Canceled).await
    }

    pub async fn pause(&self, job_id: &str) -> Result<(), AppError> {
        self.stop_job(job_id, JobStatus::Paused).await
    }

    /// Đưa job đã tạm dừng về lại hàng chờ. Không tự chạy — dispatcher lo.
    /// Giữ nguyên `queue_position` nên job quay lại đúng chỗ cũ.
    pub async fn resume(&self, job_id: &str) -> Result<(), AppError> {
        let job = self
            .db
            .get_job(job_id)?
            .ok_or_else(|| AppError::not_found("Job"))?;
        if job.status != JobStatus::Paused {
            return Err(AppError::new(
                "INVALID_JOB_STATE",
                format!("Only paused jobs can be resumed (job is {})", job.status.as_str()),
            ));
        }
        self.db.update_job_status(job_id, JobStatus::Queued, None)?;
        emit_status_changed(&self.app, job_id, JobStatus::Queued, None, None);
        self.wake.notify_one();
        Ok(())
    }
```

Thêm `clear_retry_deadline` vào `src-tauri/src/db/mod.rs`:

```rust
    /// Xoá mốc chờ thử lại và đưa bộ đếm về 0 — dùng khi người dùng can thiệp
    /// thủ công (tạm dừng, huỷ, thử lại tay).
    pub fn clear_retry_deadline(&self, job_id: &str) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE download_jobs SET next_retry_at = NULL, retry_count = 0, updated_at = ?1
             WHERE id = ?2",
            params![Utc::now().to_rfc3339(), job_id],
        )?;
        Ok(())
    }
```

- [ ] **Step 10: Cập nhật nơi khởi tạo hàng đợi**

Trong `src-tauri/src/lib.rs`, `DownloadQueue::new` giờ nhận thêm tham số:

```rust
            let db = Arc::new(Db::open(&app_data_dir.join("media-downloader.db"))?);
            let interrupted = db.reset_interrupted_jobs()?;
            let settings = db.get_settings()?;
            let queue = DownloadQueue::new(
                Arc::clone(&db),
                app.handle().clone(),
                settings.max_concurrent_downloads as usize,
            );
```

- [ ] **Step 11: Biên dịch và chạy toàn bộ test**

Run: `cd src-tauri && cargo build`
Expected: biên dịch sạch. Sửa hết lỗi trình biên dịch chỉ ra (chủ yếu là `self.` → `handles.` trong các hàm vừa chuyển thành hàm tự do).

Run: `cd src-tauri && cargo test`
Expected: PASS toàn bộ.

- [ ] **Step 12: Commit**

```bash
git add src-tauri/src
git commit -m "refactor(queue): replace semaphore with a real dispatcher

Thứ tự hàng đợi giờ nằm trong DB (sắp xếp lại được, bền qua khởi động lại),
số luồng đọc lại mỗi vòng (đổi được lúc đang chạy), và chờ thử lại là trạng
thái dữ liệu thay vì task đang ngủ (huỷ được, hiển thị được)."
```

---

### Task 6: Giới hạn tốc độ tải

**Files:**
- Modify: `src-tauri/src/downloader/queue.rs:905-988` (`build_ytdlp_args`) và nơi gọi nó (`:207`)
- Test: `src-tauri/src/downloader/queue.rs` (`mod tests` ở `:1049`)

- [ ] **Step 1: Viết test thất bại**

Thêm vào `mod tests` trong `src-tauri/src/downloader/queue.rs`:

```rust
    #[test]
    fn adds_rate_limit_flag_when_configured() {
        let job = audio_job_fixture();
        let args = build_ytdlp_args(&job, 512).expect("args build");

        let index = args
            .iter()
            .position(|a| a == "--limit-rate")
            .expect("cờ giới hạn tốc độ phải có mặt");
        assert_eq!(args[index + 1], "512K");
    }

    #[test]
    fn omits_rate_limit_flag_when_unlimited() {
        let job = audio_job_fixture();
        let args = build_ytdlp_args(&job, 0).expect("args build");

        assert!(
            !args.iter().any(|a| a == "--limit-rate"),
            "0 nghĩa là không giới hạn, không được truyền cờ"
        );
    }
```

Nếu `mod tests` chưa có helper dựng job, thêm:

```rust
    fn audio_job_fixture() -> DownloadJob {
        DownloadJob {
            id: "job-1".to_string(),
            source_url: "https://example.com/v".to_string(),
            platform: "youtube".to_string(),
            media_type: MediaType::Audio,
            audio_quality: Some("128kbps".to_string()),
            video_quality: None,
            gallery_mode: None,
            selected_gallery_indices: None,
            status: JobStatus::Queued,
            progress_percent: 0.0,
            speed_bytes_per_sec: None,
            eta_seconds: None,
            error_message: None,
            output_directory: "/tmp".to_string(),
            output_file_path: None,
            is_playlist_item: false,
            parent_playlist_id: None,
            retried_from_job_id: None,
            created_at: "2026-07-26T00:00:00Z".to_string(),
            updated_at: "2026-07-26T00:00:00Z".to_string(),
            title: None,
            playlist_title: None,
            queue_position: 0.0,
            retry_count: 0,
            next_retry_at: None,
        }
    }
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `cd src-tauri && cargo test --lib downloader::queue::tests::adds_rate_limit`
Expected: FAIL — `this function takes 1 argument but 2 arguments were supplied`.

- [ ] **Step 3: Hiện thực**

Đổi chữ ký `build_ytdlp_args` (`queue.rs:905`):

```rust
/// `rate_limit_kbps` bằng 0 nghĩa là không giới hạn. Giới hạn này áp cho từng
/// tiến trình yt-dlp, không phải tổng băng thông của ứng dụng — với N job chạy
/// song song, tổng thực tế có thể tới N lần mức này. Giao diện Cài đặt phải
/// nói rõ điều đó.
fn build_ytdlp_args(job: &DownloadJob, rate_limit_kbps: u32) -> Result<Vec<String>, AppError> {
```

Thêm ngay trước `args.push("--continue".into());` ở cuối hàm:

```rust
    if rate_limit_kbps > 0 {
        args.push("--limit-rate".into());
        args.push(format!("{rate_limit_kbps}K"));
    }
```

Tại nơi gọi trong `run_job` (`queue.rs:207`), đọc cài đặt hiện tại:

```rust
    // Đọc lại mỗi lần chạy chứ không cache: người dùng đổi giới hạn thì job
    // được khởi chạy sau đó phải dùng giá trị mới.
    let rate_limit_kbps = handles
        .db
        .get_settings()
        .map(|s| s.rate_limit_kbps)
        .unwrap_or(0);
    let args = build_ytdlp_args(&job, rate_limit_kbps)?;
```

- [ ] **Step 4: Chạy test**

Run: `cd src-tauri && cargo test --lib downloader::queue`
Expected: PASS. Các test cũ gọi `build_ytdlp_args(&job)` phải được sửa thành `build_ytdlp_args(&job, 0)`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/downloader/queue.rs
git commit -m "feat(downloader): honour the configured download rate limit"
```

---

### Task 7: Lệnh thao tác hàng loạt và sắp xếp lại thứ tự

**Files:**
- Create: `src-tauri/src/commands/queue_control.rs`
- Modify: `src-tauri/src/commands/mod.rs` (khai báo module)
- Modify: `src-tauri/src/lib.rs:34-50` (đăng ký lệnh)
- Modify: `src-tauri/src/downloader/queue.rs` (thêm `pause_all`, `resume_all`, `cancel_all`)

- [ ] **Step 1: Thêm các thao tác hàng loạt vào hàng đợi**

Thêm vào `impl DownloadQueue` trong `src-tauri/src/downloader/queue.rs`:

```rust
    /// Tạm dừng mọi tác vụ chưa kết thúc (FR-118).
    ///
    /// Gửi tín hiệu huỷ cho các job đang chạy trước, rồi mới đổi trạng thái
    /// hàng loạt trong DB — làm ngược lại thì dispatcher có thể kịp khởi chạy
    /// một job vừa được đánh dấu `paused`.
    pub async fn pause_all(&self) -> Result<Vec<String>, AppError> {
        for entry in self.running.lock().await.values() {
            let _ = entry.cancel_tx.send(true);
        }
        let changed = self.db.bulk_update_status(
            &[
                JobStatus::Queued,
                JobStatus::Downloading,
                JobStatus::FetchingMetadata,
            ],
            JobStatus::Paused,
        )?;
        for job_id in &changed {
            self.db.clear_retry_deadline(job_id)?;
            emit_status_changed(&self.app, job_id, JobStatus::Paused, None, None);
        }
        Ok(changed)
    }

    /// Đưa mọi tác vụ đang tạm dừng về hàng chờ, giữ nguyên thứ tự cũ.
    pub async fn resume_all(&self) -> Result<Vec<String>, AppError> {
        let changed = self
            .db
            .bulk_update_status(&[JobStatus::Paused], JobStatus::Queued)?;
        for job_id in &changed {
            emit_status_changed(&self.app, job_id, JobStatus::Queued, None, None);
        }
        self.wake.notify_one();
        Ok(changed)
    }

    /// Huỷ mọi tác vụ chưa kết thúc.
    pub async fn cancel_all(&self) -> Result<Vec<String>, AppError> {
        for entry in self.running.lock().await.values() {
            let _ = entry.cancel_tx.send(true);
        }
        let changed = self.db.bulk_update_status(
            &[
                JobStatus::Queued,
                JobStatus::Downloading,
                JobStatus::FetchingMetadata,
                JobStatus::Paused,
            ],
            JobStatus::Canceled,
        )?;
        for job_id in &changed {
            emit_status_changed(&self.app, job_id, JobStatus::Canceled, None, None);
        }
        Ok(changed)
    }

    /// Đặt một job vào giữa hai hàng xóm. Không đụng tới job đang chạy — chúng
    /// cứ chạy nốt, thứ tự chỉ quyết định ai được khởi chạy tiếp theo (FR-119).
    pub fn move_job(
        &self,
        job_id: &str,
        before_job_id: Option<&str>,
        after_job_id: Option<&str>,
    ) -> Result<(), AppError> {
        self.db.move_job_between(job_id, before_job_id, after_job_id)?;
        self.wake.notify_one();
        Ok(())
    }
```

- [ ] **Step 2: Tạo module lệnh**

Tạo `src-tauri/src/commands/queue_control.rs`:

```rust
//! Các lệnh tác động lên cả hàng đợi thay vì một job đơn lẻ (FR-117, FR-118).

use tauri::State;

use crate::downloader::queue::DownloadQueue;
use crate::error::AppError;

#[tauri::command]
pub async fn pause_all_jobs(queue: State<'_, DownloadQueue>) -> Result<Vec<String>, AppError> {
    queue.pause_all().await
}

#[tauri::command]
pub async fn resume_all_jobs(queue: State<'_, DownloadQueue>) -> Result<Vec<String>, AppError> {
    queue.resume_all().await
}

#[tauri::command]
pub async fn cancel_all_jobs(queue: State<'_, DownloadQueue>) -> Result<Vec<String>, AppError> {
    queue.cancel_all().await
}

/// Đặt một tác vụ vào giữa hai hàng xóm của nó sau khi người dùng thả chuột.
///
/// Giao diện gửi id hai hàng xóm chứ không gửi cả danh sách đã sắp xếp: chỉ có
/// đúng một dòng bị ghi, nên một tác vụ được thêm vào trong lúc người dùng đang
/// kéo không bị ghi đè vị trí. `None` ở một phía nghĩa là thả vào đầu (không có
/// hàng xóm phía trước) hoặc cuối (không có hàng xóm phía sau) danh sách.
#[tauri::command]
pub fn reorder_queue(
    queue: State<'_, DownloadQueue>,
    job_id: String,
    before_job_id: Option<String>,
    after_job_id: Option<String>,
) -> Result<(), AppError> {
    queue.move_job(&job_id, before_job_id.as_deref(), after_job_id.as_deref())
}
```

- [ ] **Step 3: Khai báo module và đăng ký lệnh**

Trong `src-tauri/src/commands/mod.rs` thêm:

```rust
pub mod queue_control;
```

Trong `src-tauri/src/lib.rs`, thêm vào `tauri::generate_handler![...]`:

```rust
            commands::queue_control::pause_all_jobs,
            commands::queue_control::resume_all_jobs,
            commands::queue_control::cancel_all_jobs,
            commands::queue_control::reorder_queue,
```

- [ ] **Step 4: Biên dịch**

Run: `cd src-tauri && cargo build`
Expected: biên dịch sạch.

Run: `cd src-tauri && cargo test`
Expected: PASS toàn bộ.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src
git commit -m "feat(queue): add bulk pause/resume/cancel and reorder commands"
```

---

### Task 8: Huỷ được trong giai đoạn lấy danh sách gallery

**Files:**
- Modify: `src-tauri/src/downloader/queue.rs:371-402` (giai đoạn dump trong `run_gallery_job`)

Hiện tại tín hiệu huỷ chỉ được quan sát trong vòng tải (`queue.rs:487-490`); trong lúc `dump_gallery_json` chạy, bấm Huỷ không có tác dụng gì (FR-124).

- [ ] **Step 1: Bọc lời gọi dump trong select với tín hiệu huỷ**

Trong `run_gallery_job`, thay lời gọi dump trần bằng:

```rust
    // Giai đoạn dump có thể mất vài giây với post nhiều ảnh. Không quan sát
    // tín hiệu huỷ ở đây đồng nghĩa nút Huỷ không có tác dụng suốt quãng đó
    // (FR-124).
    let dump_result = tokio::select! {
        result = gallery_dl::dump_gallery_json(&handles.app, &job.source_url) => result,
        _ = cancel_rx.changed() => {
            return Err(AppError::new("CANCELED", "Job canceled during gallery listing"));
        }
    };
```

Giữ nguyên phần xử lý `dump_result` phía sau.

- [ ] **Step 2: Biên dịch và chạy test**

Run: `cd src-tauri && cargo build && cargo test`
Expected: PASS.

- [ ] **Step 3: Kiểm chứng thủ công**

Chạy `pnpm tauri dev`, dán một link TikTok dạng slideshow, bấm Tải rồi bấm Huỷ ngay trong lúc trạng thái còn là "đang lấy thông tin". Tác vụ phải chuyển sang đã huỷ trong vòng 1 giây.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/downloader/queue.rs
git commit -m "fix(queue): honour cancellation during the gallery listing phase"
```

---

### Task 9: Đọc file danh sách URL từ phía Rust

Giữ việc đọc tệp ở Rust thay vì bật quyền `fs` cho tầng giao diện — tầng giao diện chỉ cần *một* khả năng rất hẹp (đọc một file văn bản người dùng vừa chọn), không cần quyền truy cập hệ thống tệp.

**Files:**
- Create: `src-tauri/src/commands/url_list.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Viết test thất bại**

Tạo `src-tauri/src/commands/url_list.rs` với phần test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_http_urls_one_per_line() {
        let content = "https://a.example/1\nhttp://b.example/2\n";
        assert_eq!(
            parse_url_list(content),
            vec![
                "https://a.example/1".to_string(),
                "http://b.example/2".to_string()
            ]
        );
    }

    #[test]
    fn ignores_blank_lines_comments_and_non_urls() {
        let content = "\n# ghi chú\nkhông phải url\n  https://a.example/1  \nftp://c.example/3\n";
        assert_eq!(
            parse_url_list(content),
            vec!["https://a.example/1".to_string()]
        );
    }

    #[test]
    fn drops_duplicates_but_keeps_first_seen_order() {
        let content = "https://b.example/2\nhttps://a.example/1\nhttps://b.example/2\n";
        assert_eq!(
            parse_url_list(content),
            vec![
                "https://b.example/2".to_string(),
                "https://a.example/1".to_string()
            ]
        );
    }

    #[test]
    fn finds_urls_embedded_in_surrounding_text() {
        let content = "xem cái này https://a.example/1 hay lắm";
        assert_eq!(
            parse_url_list(content),
            vec!["https://a.example/1".to_string()]
        );
    }
}
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Khai báo module trong `src-tauri/src/commands/mod.rs`:

```rust
pub mod url_list;
```

Run: `cd src-tauri && cargo test --lib commands::url_list`
Expected: FAIL — `cannot find function parse_url_list in this scope`.

- [ ] **Step 3: Hiện thực**

Thêm vào đầu `src-tauri/src/commands/url_list.rs`:

```rust
//! Đọc file danh sách URL do người dùng chọn hoặc thả vào cửa sổ (FR-105,
//! FR-106).
//!
//! Việc đọc tệp nằm ở Rust có chủ đích: tầng giao diện chỉ cần đúng khả năng
//! "đọc một file văn bản người dùng vừa chỉ định", nên không có lý do gì mở
//! quyền hệ thống tệp cho nó.

use std::path::Path;

use crate::error::AppError;

/// Chặn trên cho kích thước file danh sách. Một file 5 MB toàn URL đã là hàng
/// trăm nghìn dòng — vượt mức đó gần như chắc chắn là chọn nhầm file.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Trích mọi URL http(s) trong nội dung văn bản, bỏ trùng, giữ thứ tự xuất
/// hiện đầu tiên.
///
/// Cố ý dùng chung quy tắc với `src/lib/url-parsing.ts` phía giao diện: người
/// dùng dán vào ô nhập hay thả file vào thì phải ra cùng một kết quả.
pub fn parse_url_list(content: &str) -> Vec<String> {
    let pattern = regex::Regex::new(r"https?://[^\s\r\n]+").expect("hằng regex hợp lệ");
    let mut seen = std::collections::HashSet::new();
    let mut urls = Vec::new();

    for candidate in pattern.find_iter(content) {
        let url = candidate
            .as_str()
            .trim_end_matches([',', '.', ';', ')', ']', '"', '\''])
            .to_string();
        if seen.insert(url.clone()) {
            urls.push(url);
        }
    }
    urls
}

#[tauri::command]
pub fn read_url_list_file(path: String) -> Result<Vec<String>, AppError> {
    let path = Path::new(&path);

    let metadata = std::fs::metadata(path)
        .map_err(|err| AppError::new("FILE_UNREADABLE", format!("Cannot read file: {err}")))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(AppError::new(
            "FILE_TOO_LARGE",
            "URL list file is larger than 5 MB",
        ));
    }

    // Đọc dạng chuỗi: file nhị phân sẽ hỏng ở bước này và cho ra thông báo rõ
    // ràng, thay vì âm thầm trả về danh sách rỗng.
    let content = std::fs::read_to_string(path)
        .map_err(|_| AppError::new("FILE_NOT_TEXT", "File is not readable text"))?;

    Ok(parse_url_list(&content))
}
```

- [ ] **Step 4: Đăng ký lệnh**

Trong `src-tauri/src/lib.rs`, thêm vào `generate_handler![...]`:

```rust
            commands::url_list::read_url_list_file,
```

- [ ] **Step 5: Chạy test**

Run: `cd src-tauri && cargo test --lib commands::url_list`
Expected: PASS — 4 test.

- [ ] **Step 6: Thêm chuỗi dịch cho hai mã lỗi mới**

`src/locales/en.json`, mục `errors`:

```json
    "FILE_UNREADABLE": "Cannot read that file.",
    "FILE_TOO_LARGE": "That URL list is too large (limit 5 MB).",
    "FILE_NOT_TEXT": "That file is not a readable text file."
```

`src/locales/vi.json`, mục `errors`:

```json
    "FILE_UNREADABLE": "Không đọc được file đó.",
    "FILE_TOO_LARGE": "File danh sách URL quá lớn (giới hạn 5 MB).",
    "FILE_NOT_TEXT": "File đó không phải file văn bản đọc được."
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src src/locales
git commit -m "feat(commands): read URL list files without granting fs access to the UI"
```

---

## Nhóm B — Dọn dẹp nền móng frontend (Task 10–14)

Làm nhóm này **trước** khi thêm tính năng giao diện: `DownloadForm.tsx` đang 823 dòng và mọi tính năng mới đều chạm vào nó.

### Task 10: Kiểm tra kiểu bao phủ thư mục test

**Files:**
- Modify: `tsconfig.json:26`
- Modify: các file test đang có lỗi kiểu

- [ ] **Step 1: Xem lỗi hiện đang bị giấu**

Run: `pnpm exec tsc --noEmit -p tsconfig.json`
Expected: PASS (vì `include` chỉ có `src`).

- [ ] **Step 2: Mở rộng phạm vi kiểm tra**

Trong `tsconfig.json`, đổi dòng `include`:

```json
  "include": ["src", "tests"]
```

- [ ] **Step 3: Chạy lại để xem lỗi thật**

Run: `pnpm exec tsc --noEmit -p tsconfig.json`
Expected: FAIL — ít nhất `Property 'show_logs_tab' is missing` ở các fixture `SAMPLE_SETTINGS` trong `tests/unit/DownloadForm.test.tsx` và `tests/unit/PlaylistDetailPanel.test.tsx`.

- [ ] **Step 4: Sửa các fixture**

Trong mỗi fixture `SAMPLE_SETTINGS`, bổ sung đủ trường của `AppSettings` (kể cả 4 trường mới ở Task 4):

```typescript
const SAMPLE_SETTINGS = {
  theme: "system",
  language: "system",
  default_output_directory: "/tmp",
  show_logs_tab: false,
  max_concurrent_downloads: 3,
  rate_limit_kbps: 0,
  max_retry_attempts: 3,
  run_in_background: false,
} satisfies AppSettings;
```

Sửa mọi lỗi kiểu khác mà trình biên dịch chỉ ra.

- [ ] **Step 5: Xác nhận sạch**

Run: `pnpm exec tsc --noEmit -p tsconfig.json`
Expected: không còn lỗi.

Run: `pnpm test`
Expected: PASS toàn bộ.

- [ ] **Step 6: Commit**

```bash
git add tsconfig.json tests
git commit -m "chore(ts): type-check the tests directory"
```

---

### Task 11: Gom các hàm định dạng trùng lặp

Ba bản sao đang tồn tại: `DownloadForm.tsx:35-49`, `PlaylistDetailPanel.tsx:23-28`, `QueueList.tsx:15-19`.

**Files:**
- Create: `src/lib/format.ts`, `tests/unit/format.test.ts`
- Modify: `src/components/DownloadForm.tsx`, `src/components/PlaylistDetailPanel.tsx`, `src/components/QueueList.tsx`

- [ ] **Step 1: Viết test thất bại**

Tạo `tests/unit/format.test.ts`:

```typescript
import { describe, expect, it } from "vitest";

import { formatDuration, formatEta, formatFileSize, formatSpeed } from "@/lib/format";

describe("formatDuration", () => {
  it("shows minutes and seconds below an hour", () => {
    expect(formatDuration(75)).toBe("1:15");
  });

  it("shows hours once past one", () => {
    expect(formatDuration(3725)).toBe("1:02:05");
  });

  it("pads seconds so 1:05 never renders as 1:5", () => {
    expect(formatDuration(65)).toBe("1:05");
  });

  it("returns a placeholder when the source gave no duration", () => {
    expect(formatDuration(null)).toBe("--:--");
    expect(formatDuration(undefined)).toBe("--:--");
  });
});

describe("formatFileSize", () => {
  it("scales to the largest unit that keeps the number readable", () => {
    expect(formatFileSize(999)).toBe("999 B");
    expect(formatFileSize(1024)).toBe("1.0 KB");
    expect(formatFileSize(1024 * 1024 * 5.5)).toBe("5.5 MB");
    expect(formatFileSize(1024 * 1024 * 1024 * 2)).toBe("2.0 GB");
  });

  it("returns a placeholder when the size is unknown", () => {
    expect(formatFileSize(null)).toBe("--");
  });
});

describe("formatSpeed", () => {
  it("appends a per-second suffix", () => {
    expect(formatSpeed(1024 * 1024)).toBe("1.0 MB/s");
  });

  it("returns a placeholder when no speed has been reported yet", () => {
    expect(formatSpeed(null)).toBe("--");
  });
});

describe("formatEta", () => {
  it("reuses the duration format", () => {
    expect(formatEta(90)).toBe("1:30");
  });

  it("returns a placeholder when unknown", () => {
    expect(formatEta(null)).toBe("--:--");
  });
});
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test format`
Expected: FAIL — `Failed to resolve import "@/lib/format"`.

- [ ] **Step 3: Hiện thực**

Tạo `src/lib/format.ts`:

```typescript
/**
 * Các hàm định dạng dùng chung cho hàng đợi, lịch sử, và khu vực xem trước.
 *
 * Trước đây mỗi component tự viết một bản; ba bản đó đã trôi khác nhau (một
 * bản không đệm số 0 cho phần giây). Giữ ở một chỗ để mọi màn hình hiển thị
 * giống nhau.
 */

const PLACEHOLDER_TIME = "--:--";
const PLACEHOLDER_VALUE = "--";

/** Giây → `m:ss`, hoặc `h:mm:ss` khi từ một tiếng trở lên. */
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) {
    return PLACEHOLDER_TIME;
  }

  const total = Math.floor(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  }
  return `${minutes}:${String(secs).padStart(2, "0")}`;
}

const SIZE_UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** Byte → chuỗi có đơn vị. Đơn vị byte không có phần thập phân. */
export function formatFileSize(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes < 0) {
    return PLACEHOLDER_VALUE;
  }

  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < SIZE_UNITS.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const formatted = unitIndex === 0 ? String(Math.round(value)) : value.toFixed(1);
  return `${formatted} ${SIZE_UNITS[unitIndex]}`;
}

/** Byte mỗi giây → chuỗi tốc độ. */
export function formatSpeed(bytesPerSecond: number | null | undefined): string {
  const size = formatFileSize(bytesPerSecond);
  return size === PLACEHOLDER_VALUE ? PLACEHOLDER_VALUE : `${size}/s`;
}

/** Thời gian còn lại, dùng chung định dạng với thời lượng. */
export function formatEta(seconds: number | null | undefined): string {
  return formatDuration(seconds);
}
```

- [ ] **Step 4: Chạy test để xác nhận nó đạt**

Run: `pnpm test format`
Expected: PASS — 10 test.

- [ ] **Step 5: Thay ba bản sao bằng import**

Trong `src/components/DownloadForm.tsx`: xoá `formatDuration` và `formatFileSize` cục bộ (dòng 35-49), thêm

```typescript
import { formatDuration, formatFileSize } from "@/lib/format";
```

Trong `src/components/PlaylistDetailPanel.tsx`: xoá `formatEntryDuration` (dòng 23-28), thay mọi lời gọi bằng `formatDuration`, thêm import tương ứng.

Trong `src/components/QueueList.tsx`: xoá `formatSpeed` cục bộ (dòng 15-19), thêm import.

- [ ] **Step 6: Chạy toàn bộ test và kiểm tra kiểu**

Run: `pnpm test && pnpm exec tsc --noEmit -p tsconfig.json`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/lib/format.ts tests/unit/format.test.ts src/components
git commit -m "refactor(ui): extract shared formatting helpers"
```

---

### Task 12: Gom logic phân tích URL

**Files:**
- Create: `src/lib/url-parsing.ts`, `tests/unit/url-parsing.test.ts`
- Modify: `src/components/DownloadForm.tsx:68-88`

- [ ] **Step 1: Viết test thất bại**

Tạo `tests/unit/url-parsing.test.ts`:

```typescript
import { describe, expect, it } from "vitest";

import { dedupeUrls, extractUrlsFromText, isValidUrl } from "@/lib/url-parsing";

describe("isValidUrl", () => {
  it("accepts http and https", () => {
    expect(isValidUrl("https://example.com/v")).toBe(true);
    expect(isValidUrl("http://example.com/v")).toBe(true);
  });

  it("rejects other schemes and plain text", () => {
    expect(isValidUrl("ftp://example.com/v")).toBe(false);
    expect(isValidUrl("file:///etc/passwd")).toBe(false);
    expect(isValidUrl("just some words")).toBe(false);
    expect(isValidUrl("")).toBe(false);
  });
});

describe("extractUrlsFromText", () => {
  it("pulls urls out of surrounding prose", () => {
    expect(extractUrlsFromText("xem https://a.example/1 nhé")).toEqual(["https://a.example/1"]);
  });

  it("handles one url per line", () => {
    expect(extractUrlsFromText("https://a.example/1\nhttps://b.example/2")).toEqual([
      "https://a.example/1",
      "https://b.example/2",
    ]);
  });

  it("strips trailing punctuation that is clearly not part of the url", () => {
    expect(extractUrlsFromText("(https://a.example/1),")).toEqual(["https://a.example/1"]);
  });

  it("removes duplicates while keeping first-seen order", () => {
    expect(
      extractUrlsFromText("https://b.example/2 https://a.example/1 https://b.example/2"),
    ).toEqual(["https://b.example/2", "https://a.example/1"]);
  });

  it("returns an empty list when there is nothing url-like", () => {
    expect(extractUrlsFromText("không có link nào ở đây")).toEqual([]);
  });
});

describe("dedupeUrls", () => {
  it("reports which urls were dropped as duplicates", () => {
    const result = dedupeUrls(["https://a.example/1", "https://a.example/1", "https://b.example/2"]);
    expect(result.unique).toEqual(["https://a.example/1", "https://b.example/2"]);
    expect(result.duplicateCount).toBe(1);
  });
});
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test url-parsing`
Expected: FAIL — `Failed to resolve import "@/lib/url-parsing"`.

- [ ] **Step 3: Hiện thực**

Tạo `src/lib/url-parsing.ts`:

```typescript
/**
 * Phân tích URL từ văn bản người dùng dán, thả, hoặc từ file danh sách.
 *
 * Quy tắc ở đây phải khớp với `src-tauri/src/commands/url_list.rs`: cùng một
 * nội dung, dán vào ô nhập hay thả file vào, đều phải cho ra cùng danh sách.
 */

const URL_PATTERN = /https?:\/\/[^\s\r\n]+/g;

/** Ký tự thường dính vào cuối URL khi copy từ văn bản chạy. */
const TRAILING_NOISE = /[,.;:)\]}"']+$/;

export function isValidUrl(candidate: string): boolean {
  try {
    const parsed = new URL(candidate);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

/** Trích mọi URL http(s), bỏ trùng, giữ thứ tự xuất hiện đầu tiên. */
export function extractUrlsFromText(text: string): string[] {
  const matches = text.match(URL_PATTERN) ?? [];
  const cleaned = matches
    .map((match) => match.replace(TRAILING_NOISE, ""))
    .filter((candidate) => isValidUrl(candidate));
  return dedupeUrls(cleaned).unique;
}

export interface DedupeResult {
  unique: string[];
  duplicateCount: number;
}

/**
 * Bỏ trùng và cho biết đã bỏ bao nhiêu, để giao diện nói được với người dùng
 * rằng danh sách của họ đã bị rút ngắn (FR-107).
 */
export function dedupeUrls(urls: string[]): DedupeResult {
  const seen = new Set<string>();
  const unique: string[] = [];
  for (const url of urls) {
    if (!seen.has(url)) {
      seen.add(url);
      unique.push(url);
    }
  }
  return { unique, duplicateCount: urls.length - unique.length };
}
```

- [ ] **Step 4: Chạy test để xác nhận nó đạt**

Run: `pnpm test url-parsing`
Expected: PASS — 9 test.

- [ ] **Step 5: Dùng module mới trong DownloadForm**

Trong `src/components/DownloadForm.tsx`: xoá `isValidUrl`, `extractUrlsFromText`, và alias thừa `splitUrls` (dòng 68-88); thêm

```typescript
import { dedupeUrls, extractUrlsFromText, isValidUrl } from "@/lib/url-parsing";
```

Thay mọi lời gọi `splitUrls(...)` bằng `extractUrlsFromText(...)`.

- [ ] **Step 6: Chạy toàn bộ test**

Run: `pnpm test && pnpm exec tsc --noEmit -p tsconfig.json`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/lib/url-parsing.ts tests/unit/url-parsing.test.ts src/components/DownloadForm.tsx
git commit -m "refactor(ui): extract url parsing into a tested module"
```

---

### Task 13: Kiểm tra khớp key dịch giữa các ngôn ngữ

**Files:**
- Create: `tests/unit/locale-parity.test.ts`
- Modify: `src/locales/vi.json` (bù key thiếu)

- [ ] **Step 1: Viết test thất bại**

Tạo `tests/unit/locale-parity.test.ts`:

```typescript
import { describe, expect, it } from "vitest";

import en from "@/locales/en.json";
import vi from "@/locales/vi.json";

/**
 * Làm phẳng object lồng nhau thành danh sách đường dẫn key, để so sánh hai
 * ngôn ngữ mà không phụ thuộc thứ tự khai báo.
 */
function flattenKeys(value: unknown, prefix = ""): string[] {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return [prefix];
  }
  return Object.entries(value).flatMap(([key, child]) =>
    flattenKeys(child, prefix ? `${prefix}.${key}` : key),
  );
}

describe("locale parity (FR-133)", () => {
  const enKeys = new Set(flattenKeys(en));
  const viKeys = new Set(flattenKeys(vi));

  it("has no key present in English but missing in Vietnamese", () => {
    const missing = [...enKeys].filter((key) => !viKeys.has(key)).sort();
    expect(missing).toEqual([]);
  });

  it("has no key present in Vietnamese but missing in English", () => {
    const missing = [...viKeys].filter((key) => !enKeys.has(key)).sort();
    expect(missing).toEqual([]);
  });

  it("has no empty translation strings", () => {
    const empties = [...enKeys, ...viKeys].filter((key) => {
      const lookup = (source: unknown) =>
        key.split(".").reduce<unknown>((acc, part) => (acc as Record<string, unknown>)?.[part], source);
      return lookup(en) === "" || lookup(vi) === "";
    });
    expect(empties).toEqual([]);
  });
});
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test locale-parity`
Expected: FAIL — danh sách thiếu chứa `downloadForm.gallery_item_count_other`.

- [ ] **Step 3: Bù key thiếu**

Trong `src/locales/vi.json`, mục `downloadForm`, thêm:

```json
    "gallery_item_count_other": "{{count}} mục",
```

- [ ] **Step 4: Chạy test để xác nhận nó đạt**

Run: `pnpm test locale-parity`
Expected: PASS — 3 test.

- [ ] **Step 5: Kiểm chứng rằng test thực sự bắt được lỗi**

Thêm tạm một key chỉ có ở `en.json` (ví dụ `"__parity_probe": "x"` ở cấp ngoài cùng), chạy lại:

Run: `pnpm test locale-parity`
Expected: FAIL, liệt kê `__parity_probe`.

Xoá key thử nghiệm đó rồi chạy lại để trở về trạng thái đạt.

- [ ] **Step 6: Commit**

```bash
git add tests/unit/locale-parity.test.ts src/locales/vi.json
git commit -m "test(i18n): fail the build when locale files drift apart"
```

---

### Task 14: Tách logic dựng đầu vào tác vụ

`DownloadForm.tsx` đang dựng `CreateJobInput` ở hai chỗ (`:337-363` cho tải đơn, `:402-421` cho lô) với cách xử lý sentinel `BEST_AUDIO_QUALITY_VALUE` khác nhau nhẹ. Gom lại một chỗ trước khi chế độ lô được viết lại ở Task 18.

**Files:**
- Create: `src/lib/build-job-input.ts`, `tests/unit/build-job-input.test.ts`
- Modify: `src/components/DownloadForm.tsx`

- [ ] **Step 1: Viết test thất bại**

Tạo `tests/unit/build-job-input.test.ts`:

```typescript
import { describe, expect, it } from "vitest";

import { buildJobInput } from "@/lib/build-job-input";
import { BEST_AUDIO_QUALITY_VALUE } from "@/lib/generic-quality-options";
import type { MediaSource } from "@/types/download";

const AUDIO_PREVIEW: MediaSource = {
  source_url: "https://example.com/v",
  title: "Bài hát",
  thumbnail_url: null,
  duration_seconds: 200,
  platform: "youtube",
  is_playlist: false,
  playlist_item_count: null,
  available_video_qualities: [{ label: "1080p", filesize_bytes: 100 }],
  available_audio_formats: [{ bitrate_kbps: 128, codec: "opus", filesize_bytes: 50 }],
  is_gallery: false,
  gallery_items: [],
  playlist_entries: [],
};

const GALLERY_PREVIEW: MediaSource = {
  ...AUDIO_PREVIEW,
  is_gallery: true,
  available_video_qualities: [],
  available_audio_formats: [],
  gallery_items: [
    { url: "https://cdn/1.jpg", extension: "jpg", is_audio: false },
    { url: "https://cdn/2.jpg", extension: "jpg", is_audio: false },
    { url: "https://cdn/a.mp3", extension: "mp3", is_audio: true },
  ],
};

describe("buildJobInput", () => {
  it("builds an audio job with the chosen bitrate", () => {
    const input = buildJobInput({
      preview: AUDIO_PREVIEW,
      mediaType: "audio",
      audioQuality: "128kbps",
      videoQuality: null,
      outputDirectory: "/out",
    });

    expect(input).toEqual({
      source_url: "https://example.com/v",
      media_type: "audio",
      audio_quality: "128kbps",
      video_quality: null,
      output_directory: "/out",
    });
  });

  it("omits audio_quality when the source only offered a best-available option", () => {
    const input = buildJobInput({
      preview: AUDIO_PREVIEW,
      mediaType: "audio",
      audioQuality: BEST_AUDIO_QUALITY_VALUE,
      videoQuality: null,
      outputDirectory: "/out",
    });

    expect(input.audio_quality).toBeNull();
  });

  it("builds a video job with the chosen label", () => {
    const input = buildJobInput({
      preview: AUDIO_PREVIEW,
      mediaType: "video",
      audioQuality: null,
      videoQuality: "1080p",
      outputDirectory: "/out",
    });

    expect(input.media_type).toBe("video");
    expect(input.video_quality).toBe("1080p");
    expect(input.audio_quality).toBeNull();
  });

  it("builds a gallery job carrying the selected indices", () => {
    const input = buildJobInput({
      preview: GALLERY_PREVIEW,
      mediaType: "audio",
      audioQuality: null,
      videoQuality: null,
      outputDirectory: "/out",
      galleryMode: "images_only",
      selectedGalleryIndices: [0, 1],
    });

    expect(input.media_type).toBe("gallery");
    expect(input.gallery_mode).toBe("images_only");
    expect(input.selected_gallery_indices).toEqual([0, 1]);
  });

  it("omits the index list when every item is selected", () => {
    const input = buildJobInput({
      preview: GALLERY_PREVIEW,
      mediaType: "audio",
      audioQuality: null,
      videoQuality: null,
      outputDirectory: "/out",
      galleryMode: "files",
      selectedGalleryIndices: [0, 1, 2],
    });

    expect(input.selected_gallery_indices).toBeUndefined();
  });
});
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test build-job-input`
Expected: FAIL — `Failed to resolve import "@/lib/build-job-input"`.

- [ ] **Step 3: Hiện thực**

Tạo `src/lib/build-job-input.ts`:

```typescript
import { BEST_AUDIO_QUALITY_VALUE } from "@/lib/generic-quality-options";
import type { CreateJobInput, GalleryMode, MediaSource, MediaType } from "@/types/download";

export interface BuildJobInputArgs {
  preview: MediaSource;
  mediaType: MediaType;
  audioQuality: string | null;
  videoQuality: string | null;
  outputDirectory: string;
  galleryMode?: GalleryMode;
  selectedGalleryIndices?: number[];
}

/**
 * Một chỗ duy nhất dựng `CreateJobInput`.
 *
 * Trước đây luồng tải đơn và luồng tải lô mỗi bên dựng một kiểu, và hai bên xử
 * lý sentinel "chất lượng tốt nhất" khác nhau — nghĩa là cùng một lựa chọn của
 * người dùng có thể cho ra hai tác vụ khác nhau tuỳ theo họ dán một hay nhiều
 * link. Gom về đây để hành vi đó không thể lệch nữa.
 */
export function buildJobInput(args: BuildJobInputArgs): CreateJobInput {
  const {
    preview,
    mediaType,
    audioQuality,
    videoQuality,
    outputDirectory,
    galleryMode,
    selectedGalleryIndices,
  } = args;

  if (preview.is_gallery) {
    // Chỉ gửi danh sách chỉ số khi người dùng thực sự chọn một phần: gửi
    // "tất cả" dưới dạng danh sách đầy đủ sẽ khoá cứng số lượng item tại thời
    // điểm xem trước, trong khi backend crawl lại ngay trước lúc tải.
    const everythingSelected =
      !selectedGalleryIndices || selectedGalleryIndices.length === preview.gallery_items.length;

    return {
      source_url: preview.source_url,
      media_type: "gallery",
      audio_quality: null,
      video_quality: null,
      output_directory: outputDirectory,
      gallery_mode: galleryMode,
      ...(everythingSelected ? {} : { selected_gallery_indices: selectedGalleryIndices }),
    };
  }

  return {
    source_url: preview.source_url,
    media_type: mediaType,
    // Sentinel nghĩa là "nguồn không công bố bitrate nào" — backend hiểu
    // `null` là hãy lấy luồng tốt nhất có sẵn.
    audio_quality:
      mediaType === "audio" && audioQuality && audioQuality !== BEST_AUDIO_QUALITY_VALUE
        ? audioQuality
        : null,
    video_quality: mediaType === "video" ? videoQuality : null,
    output_directory: outputDirectory,
  };
}
```

Nếu `CreateJobInput` trong `src/types/download.ts` chưa cho phép các trường gallery là tuỳ chọn, đánh dấu `gallery_mode?: GalleryMode` và `selected_gallery_indices?: number[]`.

- [ ] **Step 4: Chạy test để xác nhận nó đạt**

Run: `pnpm test build-job-input`
Expected: PASS — 5 test.

- [ ] **Step 5: Dùng module mới trong DownloadForm**

Thay cả hai khối dựng input (`:337-363` và `:402-421`) bằng lời gọi `buildJobInput(...)`.

- [ ] **Step 6: Chạy toàn bộ test**

Run: `pnpm test && pnpm exec tsc --noEmit -p tsconfig.json`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/lib/build-job-input.ts tests/unit/build-job-input.test.ts src/components/DownloadForm.tsx src/types/download.ts
git commit -m "refactor(ui): build job input in one tested place"
```

---

## Nhóm C — Tính năng (Task 15–25)

### Task 15: Nạp lại hàng đợi từ backend khi khởi động

Đây là task sửa lỗi mất dữ liệu: `list_queue` đã tồn tại ở Rust nhưng chưa từng được gọi.

**Files:**
- Modify: `src/stores/queue-store.ts`
- Modify: `src/components/QueueList.tsx:145-147`
- Test: `tests/unit/queue-store.test.ts` (tạo mới)

- [ ] **Step 1: Viết test thất bại**

Tạo `tests/unit/queue-store.test.ts`:

```typescript
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useQueueStore } from "@/stores/queue-store";
import type { DownloadJob } from "@/types/download";

function makeJob(overrides: Partial<DownloadJob> = {}): DownloadJob {
  return {
    id: "job-1",
    source_url: "https://example.com/v",
    platform: "youtube",
    media_type: "audio",
    audio_quality: "128kbps",
    video_quality: null,
    gallery_mode: null,
    selected_gallery_indices: null,
    status: "queued",
    progress_percent: 0,
    speed_bytes_per_sec: null,
    eta_seconds: null,
    error_message: null,
    output_directory: "/out",
    output_file_path: null,
    is_playlist_item: false,
    parent_playlist_id: null,
    retried_from_job_id: null,
    created_at: "2026-07-26T00:00:00Z",
    updated_at: "2026-07-26T00:00:00Z",
    title: null,
    playlist_title: null,
    queue_position: 0,
    retry_count: 0,
    next_retry_at: null,
    ...overrides,
  };
}

describe("queue store hydration (FR-114)", () => {
  beforeEach(() => {
    useQueueStore.setState({ jobs: {} });
    vi.mocked(invoke).mockReset();
  });

  it("loads unfinished jobs from the backend", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "list_queue") {
        return [makeJob({ id: "a" }), makeJob({ id: "b", status: "paused" })];
      }
      return undefined;
    });

    await useQueueStore.getState().hydrate();

    const { jobs } = useQueueStore.getState();
    expect(Object.keys(jobs).sort()).toEqual(["a", "b"]);
    expect(jobs.b.status).toBe("paused");
  });

  it("keeps the store usable when the backend call fails", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("db locked"));

    await expect(useQueueStore.getState().hydrate()).resolves.toBeUndefined();
    expect(useQueueStore.getState().jobs).toEqual({});
  });

  it("orders jobs by queue position for display", () => {
    useQueueStore.setState({
      jobs: {
        b: makeJob({ id: "b", queue_position: 2 }),
        a: makeJob({ id: "a", queue_position: 1 }),
      },
    });

    expect(useQueueStore.getState().orderedJobs().map((job) => job.id)).toEqual(["a", "b"]);
  });
});
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test queue-store`
Expected: FAIL — `hydrate is not a function`.

- [ ] **Step 3: Hiện thực**

Trong `src/stores/queue-store.ts`, thêm vào interface store và phần tạo store:

```typescript
  /**
   * Nạp lại hàng đợi từ cơ sở dữ liệu. Trước đây store chỉ sống trong bộ nhớ
   * nên đóng ứng dụng là mất sạch hàng đợi, dù backend vẫn giữ đầy đủ dữ liệu
   * và lệnh `list_queue` đã tồn tại mà không ai gọi (FR-114).
   *
   * Không ném lỗi ra ngoài: hàng đợi trống vẫn dùng được, còn một lỗi lúc
   * khởi động mà làm hỏng cả màn hình thì tệ hơn nhiều.
   */
  hydrate: async () => {
    try {
      const jobs = await invoke<DownloadJob[]>("list_queue");
      set({
        jobs: Object.fromEntries(jobs.map((job) => [job.id, job])),
      });
    } catch (error) {
      console.error("failed to hydrate download queue", error);
    }
  },

  /** Thứ tự hiển thị = thứ tự chạy: `queue_position`, rồi `created_at`. */
  orderedJobs: () =>
    Object.values(get().jobs).sort(
      (a, b) => a.queue_position - b.queue_position || a.created_at.localeCompare(b.created_at),
    ),
```

Nhớ đổi chữ ký hàm tạo store thành `(set, get) => ({ ... })` nếu hiện tại chỉ nhận `set`.

- [ ] **Step 4: Gọi hydrate lúc mount**

Trong `src/components/QueueList.tsx`, mở rộng `useEffect` đang gọi `ensureQueueListeners()`:

```typescript
  useEffect(() => {
    ensureQueueListeners();
    void useQueueStore.getState().hydrate();
  }, []);
```

Thay mọi chỗ đang tự sắp xếp `Object.values(jobs)` bằng `orderedJobs()`.

- [ ] **Step 5: Chạy test**

Run: `pnpm test queue-store`
Expected: PASS — 3 test.

- [ ] **Step 6: Kiểm chứng thủ công**

Chạy `pnpm tauri dev`, bắt đầu tải một file lớn, đóng hẳn ứng dụng, mở lại. Tác vụ phải xuất hiện lại ở trạng thái tạm dừng và tiếp tục được.

- [ ] **Step 7: Commit**

```bash
git add src/stores/queue-store.ts src/components/QueueList.tsx tests/unit/queue-store.test.ts
git commit -m "fix(queue): restore the download queue on startup"
```

---

### Task 16: Thanh thao tác hàng loạt và đếm ngược thử lại

**Files:**
- Create: `src/components/QueueToolbar.tsx`
- Modify: `src/components/QueueList.tsx`, `src/stores/queue-store.ts`, `src/locales/*.json`
- Test: `tests/unit/QueueToolbar.test.tsx`

- [ ] **Step 1: Viết test thất bại**

Tạo `tests/unit/QueueToolbar.test.tsx`:

```typescript
import { invoke } from "@tauri-apps/api/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { QueueToolbar } from "@/components/QueueToolbar";

describe("QueueToolbar (FR-118)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue([]);
  });

  it("pauses every job", async () => {
    const user = userEvent.setup();
    render(<QueueToolbar activeCount={3} pausedCount={0} finishedCount={0} />);

    await user.click(screen.getByRole("button", { name: /pause all/i }));

    expect(invoke).toHaveBeenCalledWith("pause_all_jobs");
  });

  it("resumes every paused job", async () => {
    const user = userEvent.setup();
    render(<QueueToolbar activeCount={0} pausedCount={2} finishedCount={0} />);

    await user.click(screen.getByRole("button", { name: /resume all/i }));

    expect(invoke).toHaveBeenCalledWith("resume_all_jobs");
  });

  it("asks for confirmation before cancelling everything", async () => {
    const user = userEvent.setup();
    render(<QueueToolbar activeCount={3} pausedCount={0} finishedCount={0} />);

    await user.click(screen.getByRole("button", { name: /cancel all/i }));
    expect(invoke).not.toHaveBeenCalledWith("cancel_all_jobs");

    await user.click(screen.getByRole("button", { name: /confirm/i }));
    expect(invoke).toHaveBeenCalledWith("cancel_all_jobs");
  });

  it("disables actions that would do nothing", () => {
    render(<QueueToolbar activeCount={0} pausedCount={0} finishedCount={0} />);

    expect(screen.getByRole("button", { name: /pause all/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /resume all/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /cancel all/i })).toBeDisabled();
  });
});
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test QueueToolbar`
Expected: FAIL — không tìm thấy module.

- [ ] **Step 3: Thêm hành động hàng loạt vào store**

Trong `src/stores/queue-store.ts`:

```typescript
  pauseAll: async () => {
    await invoke("pause_all_jobs");
  },
  resumeAll: async () => {
    await invoke("resume_all_jobs");
  },
  cancelAll: async () => {
    await invoke("cancel_all_jobs");
  },
  /**
   * Chỉ dọn khỏi khung nhìn hàng đợi, không đụng tới dữ liệu: các tác vụ đã
   * kết thúc vẫn nằm nguyên trong lịch sử.
   */
  clearFinished: () => {
    set((state) => ({
      jobs: Object.fromEntries(
        Object.entries(state.jobs).filter(
          ([, job]) => !["completed", "failed", "canceled"].includes(job.status),
        ),
      ),
    }));
  },
```

Backend phát sự kiện `job:status_changed` cho từng job bị đổi, nên store tự cập nhật; ba hàm trên không cần tự set state.

- [ ] **Step 4: Hiện thực component**

Tạo `src/components/QueueToolbar.tsx`:

```typescript
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCheck, Pause, Play, Trash2, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useQueueStore } from "@/stores/queue-store";

interface QueueToolbarProps {
  activeCount: number;
  pausedCount: number;
  finishedCount: number;
}

export function QueueToolbar({ activeCount, pausedCount, finishedCount }: QueueToolbarProps) {
  const { t } = useTranslation();
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  const pauseAll = useQueueStore((state) => state.pauseAll);
  const resumeAll = useQueueStore((state) => state.resumeAll);
  const cancelAll = useQueueStore((state) => state.cancelAll);
  const clearFinished = useQueueStore((state) => state.clearFinished);

  const hasStoppable = activeCount > 0 || pausedCount > 0;

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button
        variant="outline"
        size="sm"
        disabled={activeCount === 0}
        onClick={() => void pauseAll()}
      >
        <Pause className="mr-1 size-4" />
        {t("queue.pause_all")}
      </Button>

      <Button
        variant="outline"
        size="sm"
        disabled={pausedCount === 0}
        onClick={() => void resumeAll()}
      >
        <Play className="mr-1 size-4" />
        {t("queue.resume_all")}
      </Button>

      {confirmingCancel ? (
        <>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => {
              void cancelAll();
              setConfirmingCancel(false);
            }}
          >
            <CheckCheck className="mr-1 size-4" />
            {t("queue.confirm_cancel_all")}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setConfirmingCancel(false)}>
            {t("common.cancel")}
          </Button>
        </>
      ) : (
        <Button
          variant="outline"
          size="sm"
          disabled={!hasStoppable}
          onClick={() => setConfirmingCancel(true)}
        >
          <X className="mr-1 size-4" />
          {t("queue.cancel_all")}
        </Button>
      )}

      <Button
        variant="ghost"
        size="sm"
        disabled={finishedCount === 0}
        onClick={() => clearFinished()}
      >
        <Trash2 className="mr-1 size-4" />
        {t("queue.clear_finished")}
      </Button>
    </div>
  );
}
```

- [ ] **Step 5: Thêm chuỗi dịch**

`src/locales/en.json`, mục `queue`:

```json
    "pause_all": "Pause all",
    "resume_all": "Resume all",
    "cancel_all": "Cancel all",
    "confirm_cancel_all": "Confirm cancel all",
    "clear_finished": "Clear finished",
    "retry_countdown": "Retrying in {{seconds}}s (attempt {{attempt}})"
```

`src/locales/vi.json`, mục `queue`:

```json
    "pause_all": "Tạm dừng tất cả",
    "resume_all": "Tiếp tục tất cả",
    "cancel_all": "Huỷ tất cả",
    "confirm_cancel_all": "Xác nhận huỷ tất cả",
    "clear_finished": "Dọn mục đã xong",
    "retry_countdown": "Thử lại sau {{seconds}}s (lần {{attempt}})"
```

Nếu `common.cancel` chưa có, thêm vào cả hai file.

- [ ] **Step 6: Hiển thị đếm ngược thử lại**

Trong `src/components/QueueList.tsx`, ở chỗ render trạng thái một job, thêm nhánh:

```typescript
  // Một job đang chờ thử lại là job `queued` có `next_retry_at` ở tương lai —
  // xem ghi chú trong migration 0008 về lý do không dùng một trạng thái riêng.
  const retryCountdown = useRetryCountdown(job);
  if (retryCountdown !== null) {
    return (
      <span className="text-muted-foreground text-xs">
        {t("queue.retry_countdown", { seconds: retryCountdown, attempt: job.retry_count + 1 })}
      </span>
    );
  }
```

Và thêm hook nhỏ vào cùng file:

```typescript
/**
 * Số giây còn lại tới lần thử tiếp theo, hoặc `null` nếu job không chờ thử
 * lại. Tick mỗi giây để con số thực sự đếm ngược (FR-122).
 */
function useRetryCountdown(job: DownloadJob): number | null {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!job.next_retry_at || job.status !== "queued") return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [job.next_retry_at, job.status]);

  if (!job.next_retry_at || job.status !== "queued") return null;
  const remaining = Math.ceil((new Date(job.next_retry_at).getTime() - now) / 1000);
  return remaining > 0 ? remaining : null;
}
```

- [ ] **Step 7: Gắn thanh công cụ vào danh sách hàng đợi**

Trong `QueueList.tsx`, phía trên danh sách:

```typescript
      <QueueToolbar
        activeCount={jobs.filter((job) => ["queued", "downloading", "fetching_metadata"].includes(job.status)).length}
        pausedCount={jobs.filter((job) => job.status === "paused").length}
        finishedCount={jobs.filter((job) => ["completed", "failed", "canceled"].includes(job.status)).length}
      />
```

- [ ] **Step 8: Chạy toàn bộ test**

Run: `pnpm test && pnpm exec tsc --noEmit -p tsconfig.json`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/components src/stores src/locales tests/unit/QueueToolbar.test.tsx
git commit -m "feat(queue): add bulk actions toolbar and retry countdown"
```

---

### Task 17: Kéo sắp xếp lại thứ tự hàng đợi

**Files:**
- Modify: `src/components/QueueList.tsx`, `src/stores/queue-store.ts`
- Test: `tests/unit/QueueList.test.tsx`

Dùng thuộc tính `draggable` gốc của HTML thay vì thêm thư viện: danh sách hàng đợi là một danh sách dọc phẳng, không cần khả năng của một thư viện kéo-thả đầy đủ.

- [ ] **Step 1: Viết test thất bại**

Thêm vào `tests/unit/QueueList.test.tsx`:

```typescript
  it("sends only the dropped job and its new neighbours (FR-117)", async () => {
    useQueueStore.setState({
      jobs: {
        a: makeJob({ id: "a", queue_position: 1, status: "queued" }),
        b: makeJob({ id: "b", queue_position: 2, status: "queued" }),
        c: makeJob({ id: "c", queue_position: 3, status: "queued" }),
      },
    });
    render(<QueueList />);

    const rows = screen.getAllByRole("listitem");
    // Kéo phần tử thứ ba lên vị trí đầu: nó không còn hàng xóm phía trước, và
    // hàng xóm phía sau là "a".
    fireEvent.dragStart(rows[2]);
    fireEvent.dragOver(rows[0]);
    fireEvent.drop(rows[0]);

    expect(invoke).toHaveBeenCalledWith("reorder_queue", {
      jobId: "c",
      beforeJobId: null,
      afterJobId: "a",
    });
  });

  it("passes both neighbours when dropping into the middle", async () => {
    useQueueStore.setState({
      jobs: {
        a: makeJob({ id: "a", queue_position: 1, status: "queued" }),
        b: makeJob({ id: "b", queue_position: 2, status: "queued" }),
        c: makeJob({ id: "c", queue_position: 3, status: "queued" }),
      },
    });
    render(<QueueList />);

    const rows = screen.getAllByRole("listitem");
    // Kéo "a" xuống vị trí của "b": nằm giữa "b" và "c".
    fireEvent.dragStart(rows[0]);
    fireEvent.dragOver(rows[1]);
    fireEvent.drop(rows[1]);

    expect(invoke).toHaveBeenCalledWith("reorder_queue", {
      jobId: "a",
      beforeJobId: "b",
      afterJobId: "c",
    });
  });

  it("does not let a running job be dragged", () => {
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", status: "downloading" }) },
    });
    render(<QueueList />);

    expect(screen.getAllByRole("listitem")[0]).toHaveAttribute("draggable", "false");
  });
```

Nhớ import `fireEvent` từ `@testing-library/react`.

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test QueueList`
Expected: FAIL — `invoke` chưa từng được gọi với `reorder_queue`.

- [ ] **Step 3: Thêm hành động vào store**

Trong `src/stores/queue-store.ts`:

```typescript
  /**
   * Đặt một job vào giữa hai hàng xóm mới của nó.
   *
   * Cập nhật lạc quan trước rồi mới gọi backend để thao tác kéo không bị giật.
   * Vị trí lạc quan tính đúng bằng công thức backend dùng (điểm giữa, hoặc
   * cộng/trừ 1.0 ở hai đầu) nên thứ tự hiển thị khớp ngay cả trước khi backend
   * trả lời; nếu có sai lệch thì `hydrate` ở lần mở sau đưa về trạng thái thật.
   */
  moveJob: async (
    jobId: string,
    beforeJobId: string | null,
    afterJobId: string | null,
  ) => {
    const { jobs } = get();
    const before = beforeJobId ? jobs[beforeJobId]?.queue_position : undefined;
    const after = afterJobId ? jobs[afterJobId]?.queue_position : undefined;

    const optimisticPosition =
      before !== undefined && after !== undefined
        ? (before + after) / 2
        : before !== undefined
          ? before + 1
          : after !== undefined
            ? after - 1
            : 1;

    set((state) => ({
      jobs: state.jobs[jobId]
        ? { ...state.jobs, [jobId]: { ...state.jobs[jobId], queue_position: optimisticPosition } }
        : state.jobs,
    }));

    await invoke("reorder_queue", { jobId, beforeJobId, afterJobId });
  },
```

- [ ] **Step 4: Hiện thực kéo-thả trong QueueList**

Thêm vào `src/components/QueueList.tsx`:

```typescript
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const moveJob = useQueueStore((state) => state.moveJob);

  /** Chỉ tác vụ chưa chạy mới kéo được — đổi chỗ một tác vụ đang tải
   *  không có ý nghĩa gì vì nó đã chiếm slot rồi (FR-119). */
  const isDraggable = (job: DownloadJob) => job.status === "queued" || job.status === "paused";

  const handleDrop = (targetId: string) => {
    if (!draggingId || draggingId === targetId) return;

    // Tính danh sách sau khi thả để lấy ra hai hàng xóm, rồi chỉ gửi hai id đó
    // lên backend — không gửi cả danh sách (xem `move_job_between`).
    const ids = orderedJobs.map((job) => job.id);
    const from = ids.indexOf(draggingId);
    const to = ids.indexOf(targetId);
    if (from === -1 || to === -1) return;

    ids.splice(to, 0, ids.splice(from, 1)[0]);
    const landed = ids.indexOf(draggingId);

    setDraggingId(null);
    void moveJob(draggingId, ids[landed - 1] ?? null, ids[landed + 1] ?? null);
  };
```

Trên mỗi `<li>`:

```typescript
            <li
              key={job.id}
              draggable={isDraggable(job)}
              onDragStart={() => setDraggingId(job.id)}
              onDragEnd={() => setDraggingId(null)}
              onDragOver={(event) => {
                if (draggingId) event.preventDefault();
              }}
              onDrop={() => handleDrop(job.id)}
              className={draggingId === job.id ? "opacity-50" : undefined}
            >
```

- [ ] **Step 5: Chạy test**

Run: `pnpm test QueueList`
Expected: PASS.

- [ ] **Step 6: Kiểm chứng thủ công**

Đặt số luồng = 1, xếp 4 tác vụ, kéo tác vụ cuối lên đầu, xác nhận nó là tác vụ được chạy tiếp theo. Khởi động lại ứng dụng, xác nhận thứ tự vẫn giữ nguyên.

- [ ] **Step 7: Commit**

```bash
git add src/components/QueueList.tsx src/stores/queue-store.ts tests/unit/QueueList.test.tsx
git commit -m "feat(queue): reorder pending jobs by drag and drop"
```

---

### Task 18: Chế độ tải lô có lựa chọn thật sự

Task quan trọng nhất về mặt trải nghiệm. Chế độ lô hiện tại (`DownloadForm.tsx:388-438`) **ép cứng audio**, tự lấy mức chất lượng đầu tiên, chạy tuần tự, và không hiện tiến trình từng link.

**Files:**
- Create: `src/hooks/use-batch-download.ts`, `src/components/BatchPanel.tsx`
- Create: `tests/unit/use-batch-download.test.ts`, `tests/unit/BatchPanel.test.tsx`
- Modify: `src/components/DownloadForm.tsx`, `src/locales/*.json`

- [ ] **Step 1: Viết test thất bại cho hook**

Tạo `tests/unit/use-batch-download.test.ts`:

```typescript
import { invoke } from "@tauri-apps/api/core";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useBatchDownload } from "@/hooks/use-batch-download";
import type { MediaSource } from "@/types/download";

function previewFor(url: string): MediaSource {
  return {
    source_url: url,
    title: `Title for ${url}`,
    thumbnail_url: null,
    duration_seconds: 100,
    platform: "youtube",
    is_playlist: false,
    playlist_item_count: null,
    available_video_qualities: [{ label: "720p", filesize_bytes: null }],
    available_audio_formats: [{ bitrate_kbps: 128, codec: "opus", filesize_bytes: null }],
    is_gallery: false,
    gallery_items: [],
    playlist_entries: [],
  };
}

describe("useBatchDownload (FR-101, FR-102, FR-103)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("creates one job per url using the chosen media type", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (cmd === "preview_media") return previewFor((args as { sourceUrl: string }).sourceUrl);
      if (cmd === "create_download_job") return { id: "job" };
      return undefined;
    });

    const { result } = renderHook(() => useBatchDownload());

    await act(async () => {
      await result.current.run({
        urls: ["https://a.example/1", "https://b.example/2"],
        mediaType: "video",
        outputDirectory: "/out",
      });
    });

    const created = vi
      .mocked(invoke)
      .mock.calls.filter(([cmd]) => cmd === "create_download_job")
      .map(([, args]) => (args as { input: { media_type: string } }).input.media_type);

    expect(created).toEqual(["video", "video"]);
  });

  it("keeps going when one url fails to preview", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (cmd === "preview_media") {
        const url = (args as { sourceUrl: string }).sourceUrl;
        if (url.includes("broken")) throw { code: "UNSUPPORTED_PLATFORM", message: "nope" };
        return previewFor(url);
      }
      if (cmd === "create_download_job") return { id: "job" };
      return undefined;
    });

    const { result } = renderHook(() => useBatchDownload());

    await act(async () => {
      await result.current.run({
        urls: ["https://broken.example/1", "https://ok.example/2"],
        mediaType: "audio",
        outputDirectory: "/out",
      });
    });

    await waitFor(() => {
      const items = result.current.items;
      expect(items.find((i) => i.url.includes("broken"))?.status).toBe("error");
      expect(items.find((i) => i.url.includes("ok"))?.status).toBe("created");
    });
  });

  it("never runs more previews at once than the configured limit", async () => {
    let concurrent = 0;
    let peak = 0;
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (cmd === "preview_media") {
        concurrent += 1;
        peak = Math.max(peak, concurrent);
        await new Promise((resolve) => setTimeout(resolve, 5));
        concurrent -= 1;
        return previewFor((args as { sourceUrl: string }).sourceUrl);
      }
      return { id: "job" };
    });

    const { result } = renderHook(() => useBatchDownload());
    const urls = Array.from({ length: 12 }, (_, i) => `https://example.com/${i}`);

    await act(async () => {
      await result.current.run({ urls, mediaType: "audio", outputDirectory: "/out" });
    });

    expect(peak).toBeLessThanOrEqual(4);
  });
});
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test use-batch-download`
Expected: FAIL — không tìm thấy module.

- [ ] **Step 3: Hiện thực hook**

Tạo `src/hooks/use-batch-download.ts`:

```typescript
import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { buildJobInput } from "@/lib/build-job-input";
import type { DownloadJob, MediaSource, MediaType } from "@/types/download";

/**
 * Bao nhiêu URL được xem trước cùng lúc.
 *
 * Mỗi lần xem trước là một tiến trình yt-dlp riêng, nên con số này là số tiến
 * trình chạy song song chứ không chỉ là số request. Bốn là đủ để 20 link xong
 * nhanh mà không làm ngợp máy yếu hay khiến nguồn chặn vì quá nhiều request
 * cùng lúc.
 */
const PREVIEW_CONCURRENCY = 4;

export type BatchItemStatus = "pending" | "previewing" | "created" | "error";

export interface BatchItem {
  url: string;
  status: BatchItemStatus;
  title: string | null;
  errorCode: string | null;
}

export interface RunBatchArgs {
  urls: string[];
  mediaType: MediaType;
  outputDirectory: string;
}

export function useBatchDownload() {
  const [items, setItems] = useState<BatchItem[]>([]);
  const [running, setRunning] = useState(false);

  const patch = useCallback((url: string, changes: Partial<BatchItem>) => {
    setItems((current) =>
      current.map((item) => (item.url === url ? { ...item, ...changes } : item)),
    );
  }, []);

  const run = useCallback(
    async ({ urls, mediaType, outputDirectory }: RunBatchArgs) => {
      setRunning(true);
      setItems(urls.map((url) => ({ url, status: "pending", title: null, errorCode: null })));

      // Nhóm công nhân rút việc từ một hàng đợi chung: URL nhanh không phải
      // chờ URL chậm cùng nhóm, khác với chia lô cứng theo chỉ số.
      const pending = [...urls];
      const worker = async () => {
        for (;;) {
          const url = pending.shift();
          if (!url) return;

          patch(url, { status: "previewing" });
          try {
            const preview = await invoke<MediaSource>("preview_media", { sourceUrl: url });

            // Chọn mức tốt nhất nguồn thực sự có, thay vì áp một mức cố định
            // cho mọi link (FR-019 của v1 vẫn phải được tôn trọng ở chế độ lô).
            const input = buildJobInput({
              preview,
              mediaType: preview.is_gallery ? mediaType : mediaType,
              audioQuality: preview.available_audio_formats[0]?.bitrate_kbps
                ? `${preview.available_audio_formats[0].bitrate_kbps}kbps`
                : null,
              videoQuality: preview.available_video_qualities[0]?.label ?? null,
              outputDirectory,
              galleryMode: preview.is_gallery
                ? mediaType === "audio"
                  ? "audio_only"
                  : "files"
                : undefined,
            });

            await invoke<DownloadJob>("create_download_job", { input });
            patch(url, { status: "created", title: preview.title });
          } catch (error) {
            const code =
              typeof error === "object" && error !== null && "code" in error
                ? String((error as { code: unknown }).code)
                : "DOWNLOAD_FAILED";
            patch(url, { status: "error", errorCode: code });
          }
        }
      };

      await Promise.all(
        Array.from({ length: Math.min(PREVIEW_CONCURRENCY, urls.length) }, () => worker()),
      );
      setRunning(false);
    },
    [patch],
  );

  const reset = useCallback(() => setItems([]), []);

  return { items, running, run, reset };
}
```

- [ ] **Step 4: Chạy test để xác nhận nó đạt**

Run: `pnpm test use-batch-download`
Expected: PASS — 3 test.

- [ ] **Step 5: Viết test thất bại cho panel**

Tạo `tests/unit/BatchPanel.test.tsx`:

```typescript
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { BatchPanel } from "@/components/BatchPanel";

const ITEMS = [
  { url: "https://a.example/1", status: "created" as const, title: "Bài A", errorCode: null },
  { url: "https://b.example/2", status: "error" as const, title: null, errorCode: "ACCESS_DENIED" },
  { url: "https://c.example/3", status: "previewing" as const, title: null, errorCode: null },
];

describe("BatchPanel (FR-101, FR-103)", () => {
  it("lets the user pick video instead of forcing audio", async () => {
    const user = userEvent.setup();
    const onRun = vi.fn();
    render(<BatchPanel urls={["https://a.example/1"]} items={[]} running={false} onRun={onRun} />);

    await user.click(screen.getByRole("radio", { name: /video/i }));
    await user.click(screen.getByRole("button", { name: /download all/i }));

    expect(onRun).toHaveBeenCalledWith("video");
  });

  it("shows a per-url status row", () => {
    render(<BatchPanel urls={ITEMS.map((i) => i.url)} items={ITEMS} running onRun={vi.fn()} />);

    expect(screen.getByText("Bài A")).toBeInTheDocument();
    expect(screen.getByText(/no permission|access/i)).toBeInTheDocument();
  });

  it("disables the run button while a batch is in flight", () => {
    render(<BatchPanel urls={["https://a.example/1"]} items={[]} running onRun={vi.fn()} />);

    expect(screen.getByRole("button", { name: /download all/i })).toBeDisabled();
  });
});
```

- [ ] **Step 6: Hiện thực panel**

Tạo `src/components/BatchPanel.tsx`:

```typescript
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertCircle, CheckCircle2, Loader2, Circle } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import type { BatchItem } from "@/hooks/use-batch-download";
import type { MediaType } from "@/types/download";

interface BatchPanelProps {
  urls: string[];
  items: BatchItem[];
  running: boolean;
  onRun: (mediaType: MediaType) => void;
}

const STATUS_ICON = {
  pending: Circle,
  previewing: Loader2,
  created: CheckCircle2,
  error: AlertCircle,
} as const;

export function BatchPanel({ urls, items, running, onRun }: BatchPanelProps) {
  const { t } = useTranslation();
  // Mặc định audio vì đó là nhu cầu phổ biến nhất — nhưng giờ là một mặc định
  // đổi được, chứ không phải một giá trị bị ép cứng như trước.
  const [mediaType, setMediaType] = useState<MediaType>("audio");

  return (
    <div className="space-y-4">
      <div>
        <Label className="mb-2 block">{t("downloadForm.batch_media_type")}</Label>
        <RadioGroup
          value={mediaType}
          onValueChange={(value) => setMediaType(value as MediaType)}
          className="flex gap-4"
          disabled={running}
        >
          <div className="flex items-center gap-2">
            <RadioGroupItem value="audio" id="batch-audio" />
            <Label htmlFor="batch-audio">{t("downloadForm.audio_only")}</Label>
          </div>
          <div className="flex items-center gap-2">
            <RadioGroupItem value="video" id="batch-video" />
            <Label htmlFor="batch-video">{t("downloadForm.full_video")}</Label>
          </div>
        </RadioGroup>
        <p className="text-muted-foreground mt-1 text-xs">
          {t("downloadForm.batch_quality_hint")}
        </p>
      </div>

      {items.length > 0 && (
        <ul className="max-h-64 space-y-1 overflow-y-auto text-sm">
          {items.map((item) => {
            const Icon = STATUS_ICON[item.status];
            return (
              <li key={item.url} className="flex items-center gap-2">
                <Icon
                  className={`size-4 shrink-0 ${item.status === "previewing" ? "animate-spin" : ""} ${
                    item.status === "error" ? "text-destructive" : ""
                  }`}
                />
                <span className="truncate">{item.title ?? item.url}</span>
                {item.errorCode && (
                  <span className="text-destructive ml-auto shrink-0 text-xs">
                    {t(`errors.${item.errorCode}`, { defaultValue: item.errorCode })}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}

      <Button disabled={running || urls.length === 0} onClick={() => onRun(mediaType)}>
        {t("downloadForm.download_all", { count: urls.length })}
      </Button>
    </div>
  );
}
```

- [ ] **Step 7: Thay chế độ lô cũ trong DownloadForm**

Trong `src/components/DownloadForm.tsx`: xoá `handleDownloadAllBatch` (`:388-438`) và khối hiển thị `batchErrors` (`:754-762`); thay bằng:

```typescript
  const batch = useBatchDownload();

  // ... trong JSX, thay nhánh chế độ lô:
  {isBatchMode && (
    <BatchPanel
      urls={urls}
      items={batch.items}
      running={batch.running}
      onRun={(mediaType) =>
        void batch.run({ urls, mediaType, outputDirectory: effectiveOutputDirectory })
      }
    />
  )}
```

- [ ] **Step 8: Thêm chuỗi dịch**

`src/locales/en.json`, mục `downloadForm`:

```json
    "batch_media_type": "Download these as",
    "batch_quality_hint": "Each link gets the best quality its source actually offers.",
    "download_all_one": "Download 1 link",
    "download_all_other": "Download {{count}} links"
```

`src/locales/vi.json`, mục `downloadForm`:

```json
    "batch_media_type": "Tải các link này dưới dạng",
    "batch_quality_hint": "Mỗi link sẽ lấy mức chất lượng tốt nhất mà nguồn của nó thực sự có.",
    "download_all_one": "Tải 1 link",
    "download_all_other": "Tải {{count}} link"
```

- [ ] **Step 9: Chạy toàn bộ test**

Run: `pnpm test && pnpm exec tsc --noEmit -p tsconfig.json`
Expected: PASS. Test `locale-parity` sẽ bắt ngay nếu quên thêm key ở một ngôn ngữ.

- [ ] **Step 10: Commit**

```bash
git add src/hooks/use-batch-download.ts src/components/BatchPanel.tsx src/components/DownloadForm.tsx src/locales tests/unit
git commit -m "feat(download): batch mode with real media type choice and per-url status"
```

---

### Task 19: Kéo-thả URL và file danh sách vào cửa sổ

**Files:**
- Create: `src/hooks/use-file-drop.ts`
- Modify: `src/components/DownloadForm.tsx`, `src-tauri/src/lib.rs` (nếu cần bật), `src/locales/*.json`
- Test: `tests/unit/use-file-drop.test.ts`

- [ ] **Step 1: Kiểm tra sự kiện thả tệp đã bật chưa**

Tauri 2 bật `dragDropEnabled` mặc định cho cửa sổ. Xác nhận trong `src-tauri/tauri.conf.json` mục `app.windows[0]` **không có** `"dragDropEnabled": false`. Nếu có, xoá dòng đó.

- [ ] **Step 2: Viết test thất bại**

Tạo `tests/unit/use-file-drop.test.ts`:

```typescript
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useFileDrop } from "@/hooks/use-file-drop";

type DropHandler = (event: { payload: { paths: string[] } }) => void;

describe("useFileDrop (FR-104, FR-105)", () => {
  let handler: DropHandler | null = null;

  beforeEach(() => {
    handler = null;
    vi.mocked(invoke).mockReset();
    vi.mocked(listen).mockImplementation(async (_event, callback) => {
      handler = callback as DropHandler;
      return () => {};
    });
  });

  it("reads urls out of a dropped text file", async () => {
    vi.mocked(invoke).mockResolvedValue(["https://a.example/1", "https://b.example/2"]);
    const onUrls = vi.fn();
    renderHook(() => useFileDrop(onUrls));

    await waitFor(() => expect(handler).not.toBeNull());
    handler!({ payload: { paths: ["/tmp/list.txt"] } });

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("read_url_list_file", { path: "/tmp/list.txt" });
      expect(onUrls).toHaveBeenCalledWith(["https://a.example/1", "https://b.example/2"]);
    });
  });

  it("ignores files that are not text lists", async () => {
    const onUrls = vi.fn();
    renderHook(() => useFileDrop(onUrls));

    await waitFor(() => expect(handler).not.toBeNull());
    handler!({ payload: { paths: ["/tmp/photo.png"] } });

    await waitFor(() => expect(invoke).not.toHaveBeenCalled());
    expect(onUrls).not.toHaveBeenCalled();
  });

  it("merges urls from several dropped files", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce(["https://a.example/1"])
      .mockResolvedValueOnce(["https://b.example/2"]);
    const onUrls = vi.fn();
    renderHook(() => useFileDrop(onUrls));

    await waitFor(() => expect(handler).not.toBeNull());
    handler!({ payload: { paths: ["/tmp/a.txt", "/tmp/b.txt"] } });

    await waitFor(() =>
      expect(onUrls).toHaveBeenCalledWith(["https://a.example/1", "https://b.example/2"]),
    );
  });
});
```

- [ ] **Step 3: Chạy test để xác nhận nó thất bại**

Run: `pnpm test use-file-drop`
Expected: FAIL — không tìm thấy module.

- [ ] **Step 4: Hiện thực**

Tạo `src/hooks/use-file-drop.ts`:

```typescript
import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { dedupeUrls } from "@/lib/url-parsing";

/** Phần mở rộng được coi là danh sách URL dạng văn bản. */
const URL_LIST_EXTENSIONS = [".txt", ".list", ".csv"];

/**
 * Nhận file danh sách URL thả vào cửa sổ (FR-105).
 *
 * Việc đọc tệp nằm ở Rust (`read_url_list_file`) chứ không ở đây: tầng giao
 * diện không được cấp quyền hệ thống tệp, và cũng không cần.
 *
 * URL kéo thẳng từ trình duyệt đi theo đường khác — chúng tới dưới dạng thao
 * tác dán/thả văn bản trên chính ô nhập, được xử lý ở `DownloadForm`.
 */
export function useFileDrop(onUrls: (urls: string[]) => void) {
  useEffect(() => {
    let disposed = false;
    const unlistenPromise = listen<{ paths: string[] }>("tauri://drag-drop", async (event) => {
      const listFiles = event.payload.paths.filter((path) =>
        URL_LIST_EXTENSIONS.some((extension) => path.toLowerCase().endsWith(extension)),
      );
      if (listFiles.length === 0) return;

      const collected: string[] = [];
      for (const path of listFiles) {
        try {
          const urls = await invoke<string[]>("read_url_list_file", { path });
          collected.push(...urls);
        } catch (error) {
          console.error("failed to read dropped url list", path, error);
        }
      }

      if (!disposed && collected.length > 0) {
        onUrls(dedupeUrls(collected).unique);
      }
    });

    return () => {
      disposed = true;
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [onUrls]);
}
```

- [ ] **Step 5: Gắn vào DownloadForm**

Trong `src/components/DownloadForm.tsx`:

```typescript
  const handleDroppedUrls = useCallback(
    (dropped: string[]) => {
      setRawInput((current) => {
        const merged = dedupeUrls([...extractUrlsFromText(current), ...dropped]);
        return merged.unique.join("\n");
      });
      toast.success(t("downloadForm.dropped_urls", { count: dropped.length }));
    },
    [t],
  );

  useFileDrop(handleDroppedUrls);
```

- [ ] **Step 6: Thêm nút chọn file danh sách**

Cạnh nút chọn thư mục, thêm:

```typescript
      <Button
        variant="outline"
        onClick={async () => {
          const selected = await open({
            multiple: false,
            filters: [{ name: "URL list", extensions: ["txt", "list", "csv"] }],
          });
          if (typeof selected === "string") {
            const urls = await invoke<string[]>("read_url_list_file", { path: selected });
            handleDroppedUrls(urls);
          }
        }}
      >
        {t("downloadForm.import_url_list")}
      </Button>
```

- [ ] **Step 7: Thêm chuỗi dịch**

`en.json` mục `downloadForm`:

```json
    "import_url_list": "Import URL list",
    "dropped_urls_one": "Added 1 link from the file",
    "dropped_urls_other": "Added {{count}} links from the file"
```

`vi.json` mục `downloadForm`:

```json
    "import_url_list": "Nhập file danh sách URL",
    "dropped_urls_one": "Đã thêm 1 link từ file",
    "dropped_urls_other": "Đã thêm {{count}} link từ file"
```

- [ ] **Step 8: Chạy test**

Run: `pnpm test && pnpm exec tsc --noEmit -p tsconfig.json`
Expected: PASS.

- [ ] **Step 9: Kiểm chứng thủ công**

Chạy `pnpm tauri dev`. Tạo một file `.txt` chứa 3 URL, kéo thả vào cửa sổ, xác nhận cả 3 xuất hiện trong ô nhập. Kéo một file ảnh vào, xác nhận không có gì xảy ra.

- [ ] **Step 10: Commit**

```bash
git add src/hooks/use-file-drop.ts src/components/DownloadForm.tsx src/locales tests/unit/use-file-drop.test.ts
git commit -m "feat(download): accept dropped and imported URL list files"
```

---

### Task 20: Lưới chọn ảnh hiển thị đủ mục

Lỗi hiện tại: lưới chỉ render 24 mục đầu (`DownloadForm.tsx:627`) nhưng mặc định chọn **tất cả**, nên người dùng không thấy và không bỏ chọn được các mục từ thứ 25 trở đi mà chúng vẫn được tải (FR-134).

**Files:**
- Create: `src/components/GalleryItemPicker.tsx`, `tests/unit/GalleryItemPicker.test.tsx`
- Modify: `src/components/DownloadForm.tsx:595-677`

- [ ] **Step 1: Viết test thất bại**

Tạo `tests/unit/GalleryItemPicker.test.tsx`:

```typescript
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { GalleryItemPicker } from "@/components/GalleryItemPicker";
import type { GalleryItemPreview } from "@/types/download";

function items(count: number): GalleryItemPreview[] {
  return Array.from({ length: count }, (_, i) => ({
    url: `https://cdn/${i}.jpg`,
    extension: "jpg",
    is_audio: false,
  }));
}

describe("GalleryItemPicker (FR-134)", () => {
  it("renders every selectable item, not just the first 24", () => {
    render(
      <GalleryItemPicker items={items(30)} selectedIndices={[]} onChange={vi.fn()} />,
    );

    expect(screen.getAllByRole("checkbox")).toHaveLength(30);
  });

  it("toggles an item by its original index", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <GalleryItemPicker items={items(3)} selectedIndices={[0, 1, 2]} onChange={onChange} />,
    );

    await user.click(screen.getAllByRole("checkbox")[1]);

    expect(onChange).toHaveBeenCalledWith([0, 2]);
  });

  it("selects and clears everything", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<GalleryItemPicker items={items(3)} selectedIndices={[0]} onChange={onChange} />);

    await user.click(screen.getByRole("button", { name: /select all/i }));
    expect(onChange).toHaveBeenCalledWith([0, 1, 2]);

    await user.click(screen.getByRole("button", { name: /select none/i }));
    expect(onChange).toHaveBeenCalledWith([]);
  });

  it("skips audio tracks, whose inclusion is governed by gallery mode", () => {
    const mixed: GalleryItemPreview[] = [
      { url: "https://cdn/0.jpg", extension: "jpg", is_audio: false },
      { url: "https://cdn/1.mp3", extension: "mp3", is_audio: true },
    ];
    render(<GalleryItemPicker items={mixed} selectedIndices={[0, 1]} onChange={vi.fn()} />);

    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
  });
});
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test GalleryItemPicker`
Expected: FAIL — không tìm thấy module.

- [ ] **Step 3: Hiện thực**

Tạo `src/components/GalleryItemPicker.tsx`:

```typescript
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type { GalleryItemPreview } from "@/types/download";

interface GalleryItemPickerProps {
  items: GalleryItemPreview[];
  selectedIndices: number[];
  onChange: (indices: number[]) => void;
}

/**
 * Lưới chọn ảnh cho nội dung dạng thư viện.
 *
 * Chỉ số truyền ra là chỉ số trong mảng `gallery_items` **gốc**, không phải
 * trong danh sách ảnh đã lọc — backend áp các chỉ số này lên một lần crawl mới
 * theo đúng thứ tự đó.
 *
 * Bản trước cắt lưới ở 24 mục nhưng vẫn chọn sẵn tất cả, nên các mục từ thứ 25
 * trở đi bị tải mà người dùng không hề thấy chúng (FR-134). Ở đây render đủ và
 * cho cuộn.
 */
export function GalleryItemPicker({ items, selectedIndices, onChange }: GalleryItemPickerProps) {
  const { t } = useTranslation();

  // Track âm thanh không nằm trong lưới: việc có lấy nó hay không do chế độ
  // thư viện quyết định, không do người dùng tick từng cái.
  const imageEntries = items
    .map((item, originalIndex) => ({ item, originalIndex }))
    .filter(({ item }) => !item.is_audio);

  const selected = new Set(selectedIndices);

  const toggle = (originalIndex: number) => {
    const next = new Set(selected);
    if (next.has(originalIndex)) {
      next.delete(originalIndex);
    } else {
      next.add(originalIndex);
    }
    onChange([...next].sort((a, b) => a - b));
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <span className="text-muted-foreground text-sm">
          {t("downloadForm.gallery_selected", {
            selected: imageEntries.filter(({ originalIndex }) => selected.has(originalIndex)).length,
            total: imageEntries.length,
          })}
        </span>
        <Button
          variant="ghost"
          size="sm"
          className="ml-auto"
          onClick={() => onChange(imageEntries.map(({ originalIndex }) => originalIndex))}
        >
          {t("downloadForm.select_all")}
        </Button>
        <Button variant="ghost" size="sm" onClick={() => onChange([])}>
          {t("downloadForm.select_none")}
        </Button>
      </div>

      <div className="grid max-h-96 grid-cols-4 gap-2 overflow-y-auto sm:grid-cols-6">
        {imageEntries.map(({ item, originalIndex }) => (
          <label
            key={item.url}
            className="relative aspect-square cursor-pointer overflow-hidden rounded border"
          >
            <input
              type="checkbox"
              className="absolute left-1 top-1 z-10 size-4"
              checked={selected.has(originalIndex)}
              onChange={() => toggle(originalIndex)}
            />
            <img
              src={item.url}
              alt=""
              loading="lazy"
              className={`size-full object-cover transition-opacity ${
                selected.has(originalIndex) ? "opacity-100" : "opacity-40"
              }`}
            />
          </label>
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Thay khối IIFE trong DownloadForm**

Xoá khối lưới ảnh inline (`DownloadForm.tsx:597-666`), thay bằng:

```typescript
              <GalleryItemPicker
                items={preview.gallery_items}
                selectedIndices={selectedGalleryIndices}
                onChange={setSelectedGalleryIndices}
              />
```

- [ ] **Step 5: Chạy test**

Run: `pnpm test && pnpm exec tsc --noEmit -p tsconfig.json`
Expected: PASS. Nếu key `downloadForm.select_all` / `select_none` / `gallery_selected` chưa có, thêm vào cả hai file locale — `locale-parity` sẽ nhắc.

- [ ] **Step 6: Commit**

```bash
git add src/components tests/unit/GalleryItemPicker.test.tsx src/locales
git commit -m "fix(gallery): render every selectable item instead of the first 24"
```

---

### Task 21: Giao diện cài đặt cho số luồng, tốc độ, chạy nền

**Files:**
- Modify: `src/pages/Settings.tsx`, `src/locales/*.json`
- Modify: `src-tauri/src/commands/settings.rs` (áp số luồng lên hàng đợi đang chạy)
- Test: `tests/unit/Settings.test.tsx` (tạo mới)

- [ ] **Step 1: Áp số luồng mới lên hàng đợi ngay lập tức**

Trong `src-tauri/src/commands/settings.rs`, đổi chữ ký lệnh để nhận thêm hàng đợi và gọi `set_max_concurrent`:

```rust
#[tauri::command]
pub fn update_settings(
    db: State<Arc<Db>>,
    queue: State<DownloadQueue>,
    patch: UpdateSettingsInput,
) -> Result<AppSettings, AppError> {
    // ... phần merge như cũ ...
    db.update_settings(&current)?;

    // Áp ngay thay vì đợi khởi động lại (FR-113). `set_max_concurrent` đánh
    // thức dispatcher nên việc tăng số luồng có hiệu lực trong vòng một nhịp.
    queue.set_max_concurrent(current.max_concurrent_downloads as usize);

    Ok(current)
}
```

Thêm `use crate::downloader::queue::DownloadQueue;`.

- [ ] **Step 2: Viết test thất bại cho giao diện**

Tạo `tests/unit/Settings.test.tsx`:

```typescript
import { invoke } from "@tauri-apps/api/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { Settings } from "@/pages/Settings";
import type { AppSettings } from "@/types/settings";

const SETTINGS: AppSettings = {
  theme: "system",
  language: "system",
  default_output_directory: "/out",
  show_logs_tab: false,
  max_concurrent_downloads: 3,
  rate_limit_kbps: 0,
  max_retry_attempts: 3,
  run_in_background: false,
};

describe("Settings page (FR-112, FR-126, FR-127)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "get_settings") return SETTINGS;
      if (cmd === "update_settings") return SETTINGS;
      return undefined;
    });
  });

  it("saves a new concurrency value", async () => {
    const user = userEvent.setup();
    render(<Settings />);

    const input = await screen.findByLabelText(/concurrent downloads/i);
    await user.clear(input);
    await user.type(input, "6");
    await user.tab();

    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: expect.objectContaining({ max_concurrent_downloads: 6 }),
    });
  });

  it("saves a rate limit", async () => {
    const user = userEvent.setup();
    render(<Settings />);

    const input = await screen.findByLabelText(/speed limit/i);
    await user.clear(input);
    await user.type(input, "512");
    await user.tab();

    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: expect.objectContaining({ rate_limit_kbps: 512 }),
    });
  });

  it("explains that the speed limit is per download, not total", async () => {
    render(<Settings />);
    expect(await screen.findByText(/per download/i)).toBeInTheDocument();
  });

  it("toggles background mode", async () => {
    const user = userEvent.setup();
    render(<Settings />);

    await user.click(await screen.findByRole("switch", { name: /keep running/i }));

    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: expect.objectContaining({ run_in_background: true }),
    });
  });
});
```

- [ ] **Step 3: Chạy test để xác nhận nó thất bại**

Run: `pnpm test Settings`
Expected: FAIL — không tìm thấy các control.

- [ ] **Step 4: Hiện thực**

Trong `src/pages/Settings.tsx`, thêm ba khối, theo đúng mẫu `handleToggleShowLogsTab` sẵn có:

```typescript
      <div className="space-y-2">
        <Label htmlFor="max-concurrent">{t("settings.max_concurrent_label")}</Label>
        <Input
          id="max-concurrent"
          type="number"
          min={1}
          max={8}
          defaultValue={settings.max_concurrent_downloads}
          onBlur={(event) =>
            void update({
              max_concurrent_downloads: Number(event.target.value) || 3,
            })
          }
          className="w-24"
        />
        <p className="text-muted-foreground text-sm">{t("settings.max_concurrent_hint")}</p>
      </div>

      <div className="space-y-2">
        <Label htmlFor="rate-limit">{t("settings.rate_limit_label")}</Label>
        <Input
          id="rate-limit"
          type="number"
          min={0}
          defaultValue={settings.rate_limit_kbps}
          onBlur={(event) => void update({ rate_limit_kbps: Number(event.target.value) || 0 })}
          className="w-32"
        />
        <p className="text-muted-foreground text-sm">{t("settings.rate_limit_hint")}</p>
      </div>

      <div className="flex items-center justify-between">
        <div>
          <Label htmlFor="run-in-background">{t("settings.background_label")}</Label>
          <p className="text-muted-foreground text-sm">{t("settings.background_hint")}</p>
        </div>
        <Switch
          id="run-in-background"
          checked={settings.run_in_background}
          onCheckedChange={(checked) => void update({ run_in_background: checked })}
        />
      </div>
```

- [ ] **Step 5: Thêm chuỗi dịch**

`en.json`, mục `settings`:

```json
    "max_concurrent_label": "Concurrent downloads",
    "max_concurrent_hint": "How many downloads run at the same time (1–8). More is not always faster — sources rate-limit too.",
    "rate_limit_label": "Speed limit (KB/s)",
    "rate_limit_hint": "0 means unlimited. This limit applies per download, not to the app total — with 3 running at once the combined speed can be up to 3× this number.",
    "background_label": "Keep running in the background",
    "background_hint": "Closing the window minimises to the system tray and downloads keep going."
```

`vi.json`, mục `settings`:

```json
    "max_concurrent_label": "Số tải song song",
    "max_concurrent_hint": "Bao nhiêu tác vụ chạy cùng lúc (1–8). Nhiều hơn không phải lúc nào cũng nhanh hơn — nguồn cũng giới hạn tần suất.",
    "rate_limit_label": "Giới hạn tốc độ (KB/s)",
    "rate_limit_hint": "0 là không giới hạn. Giới hạn này áp cho từng tác vụ, không phải tổng của ứng dụng — chạy 3 tác vụ cùng lúc thì tổng có thể tới 3 lần con số này.",
    "background_label": "Tiếp tục chạy nền",
    "background_hint": "Đóng cửa sổ sẽ thu về khay hệ thống và các tác vụ vẫn tải tiếp."
```

Chuỗi gợi ý về giới hạn tốc độ là bắt buộc, không phải trang trí: spec đã ghi rõ phải nói cho người dùng biết đây là giới hạn theo tác vụ.

- [ ] **Step 6: Chạy test**

Run: `pnpm test && pnpm exec tsc --noEmit -p tsconfig.json && cd src-tauri && cargo test`
Expected: PASS.

- [ ] **Step 7: Kiểm chứng thủ công (SC-103)**

Đặt số luồng = 1, xếp 3 tác vụ, xác nhận chỉ 1 chạy. Trong lúc đang tải, tăng lên 3 — hai tác vụ chờ phải bắt đầu chạy trong vòng 2 giây mà không cần khởi động lại.

- [ ] **Step 8: Commit**

```bash
git add src/pages/Settings.tsx src/locales src-tauri/src/commands/settings.rs tests/unit/Settings.test.tsx
git commit -m "feat(settings): expose concurrency, rate limit and background mode"
```

---

### Task 22: Dọn các chuỗi viết cứng bỏ qua i18n

**Files:**
- Modify: `src/components/DownloadForm.tsx:476,478,483,501,541`
- Modify: `src/pages/History.tsx:18,28,36-39`
- Modify: `src/locales/en.json`, `src/locales/vi.json`
- Test: `tests/unit/no-hardcoded-strings.test.ts` (tạo mới)

- [ ] **Step 1: Viết test thất bại**

Tạo `tests/unit/no-hardcoded-strings.test.ts`:

```typescript
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Bắt các chuỗi hiển thị bị viết thẳng vào JSX thay vì đi qua `t()` (FR-132).
 *
 * Heuristic, không phải bộ phân tích cú pháp đầy đủ: nó tìm các chuỗi có dấu
 * tiếng Việt hoặc các từ tiếng Anh dạng câu nằm trong dấu nháy, mà không nằm
 * trong một lời gọi `t(...)`. Đủ tốt để chặn hồi quy, và nếu báo nhầm thì thêm
 * vào danh sách miễn trừ có kèm lý do.
 */
const UI_DIRECTORIES = ["src/components", "src/pages"];

/** Có dấu tiếng Việt — gần như chắc chắn là văn bản cho người dùng đọc. */
const VIETNAMESE = /[àáảãạăằắẳẵặâầấẩẫậèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵđ]/i;

function collectFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) return collectFiles(full);
    return full.endsWith(".tsx") ? [full] : [];
  });
}

describe("no hard-coded UI strings (FR-132)", () => {
  const files = UI_DIRECTORIES.flatMap(collectFiles).filter(
    // Các primitive shadcn/ui không chứa văn bản của riêng chúng.
    (file) => !file.includes("/ui/"),
  );

  it.each(files)("%s has no Vietnamese text outside t()", (file) => {
    const offenders = readFileSync(file, "utf8")
      .split("\n")
      .map((line, index) => ({ line: line.trim(), number: index + 1 }))
      .filter(({ line }) => VIETNAMESE.test(line) && !line.includes("t(") && !line.startsWith("//") && !line.startsWith("*"))
      .map(({ line, number }) => `${number}: ${line}`);

    expect(offenders).toEqual([]);
  });
});
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `pnpm test no-hardcoded-strings`
Expected: FAIL — liệt kê các dòng ở `DownloadForm.tsx` (`Làm sạch link`, `Đã tự động lọc...`, `Loại bỏ ký tự thừa...`).

- [ ] **Step 3: Thêm chuỗi dịch còn thiếu**

`en.json`, mục `downloadForm`:

```json
    "clean_links": "Clean up links ({{count}} valid)",
    "clean_links_title": "Strip extra characters and keep only valid links",
    "auto_filtered_one": "Automatically kept 1 valid link",
    "auto_filtered_other": "Automatically kept {{count}} valid links",
    "single_url": "Single URL",
    "url_count_other": "{{count}} URLs",
    "supported_label": "Supported:"
```

`vi.json`, mục `downloadForm`:

```json
    "clean_links": "Làm sạch link ({{count}} link hợp lệ)",
    "clean_links_title": "Loại bỏ ký tự thừa, chỉ giữ lại link hợp lệ",
    "auto_filtered_one": "Đã tự động lọc 1 link hợp lệ",
    "auto_filtered_other": "Đã tự động lọc {{count}} link hợp lệ",
    "single_url": "Một URL",
    "url_count_other": "{{count}} URL",
    "supported_label": "Hỗ trợ:"
```

`en.json`, mục `history`:

```json
    "subtitle": "Manage and access all your downloaded media files",
    "search_placeholder": "Search downloads...",
    "tab_all": "All Downloads",
    "tab_completed": "Completed",
    "tab_failed": "Failed",
    "tab_canceled": "Canceled"
```

`vi.json`, mục `history`:

```json
    "subtitle": "Quản lý và mở lại mọi tệp media đã tải",
    "search_placeholder": "Tìm trong các bản đã tải...",
    "tab_all": "Tất cả",
    "tab_completed": "Hoàn tất",
    "tab_failed": "Thất bại",
    "tab_canceled": "Đã huỷ"
```

- [ ] **Step 4: Thay các chuỗi viết cứng bằng lời gọi t()**

Trong `DownloadForm.tsx`, ví dụ cho dòng 478:

```typescript
        {t("downloadForm.clean_links", { count: urls.length })}
```

Làm tương tự cho các dòng 476, 483, 501, 541. Trong `History.tsx`, thay các dòng 18, 28, 36-39.

- [ ] **Step 5: Chạy test**

Run: `pnpm test`
Expected: PASS — cả `no-hardcoded-strings` lẫn `locale-parity`.

- [ ] **Step 6: Kiểm chứng thủ công (SC-007 của v1)**

Chạy ứng dụng, chuyển sang tiếng Anh, xác nhận không còn chữ tiếng Việt nào sót lại trên màn hình Home và History.

- [ ] **Step 7: Commit**

```bash
git add src/components src/pages src/locales tests/unit/no-hardcoded-strings.test.ts
git commit -m "fix(i18n): route every user-facing string through translations"
```

---

### Task 23: Thông báo hệ thống khi cửa sổ không hiển thị

**Files:**
- Modify: `src-tauri/Cargo.toml`, `package.json`, `src-tauri/capabilities/default.json`
- Create: `src-tauri/src/notify.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/src/downloader/queue.rs`

- [ ] **Step 1: Cài plugin thông báo**

```bash
cd src-tauri && cargo add tauri-plugin-notification --features windows7-compat
cd .. && pnpm add @tauri-apps/plugin-notification
```

- [ ] **Step 2: Đăng ký plugin và quyền**

Trong `src-tauri/src/lib.rs`:

```rust
        .plugin(tauri_plugin_notification::init())
```

Trong `src-tauri/capabilities/default.json`, thêm vào mảng `permissions`:

```json
    "notification:default"
```

- [ ] **Step 3: Viết module gửi thông báo**

Tạo `src-tauri/src/notify.rs`:

```rust
//! Thông báo hệ thống cho tác vụ kết thúc (FR-128).
//!
//! Chỉ gửi khi cửa sổ chính không hiển thị. Bắn thông báo trong lúc người dùng
//! đang nhìn thẳng vào hàng đợi là nhiễu, không phải thông tin.

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

/// Cửa sổ chính có đang thực sự trước mặt người dùng không.
///
/// Ẩn, thu nhỏ, hoặc không có cửa sổ nào đều tính là không hiển thị. Nếu không
/// truy vấn được trạng thái, coi như không hiển thị: thà gửi thừa một thông
/// báo còn hơn im lặng khi người dùng đang chờ.
fn window_is_visible(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    let visible = window.is_visible().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(false);
    let focused = window.is_focused().unwrap_or(false);
    visible && !minimized && focused
}

pub fn notify_job_finished(app: &AppHandle, title: &str, body: &str) {
    if window_is_visible(app) {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}
```

Khai báo trong `src-tauri/src/lib.rs`: `mod notify;`

- [ ] **Step 4: Gọi khi tác vụ kết thúc**

Trong `src-tauri/src/downloader/queue.rs`, ở nhánh hoàn tất của `run_job` (ngay sau khi đặt trạng thái `Completed`):

```rust
    crate::notify::notify_job_finished(
        &handles.app,
        "Download complete",
        job.title.as_deref().unwrap_or(&job.source_url),
    );
```

Và trong `finish_job`, ở nhánh thất bại vĩnh viễn:

```rust
    crate::notify::notify_job_finished(&handles.app, "Download failed", &err.message);
```

Tiêu đề thông báo để tiếng Anh vì tầng Rust không có ngữ cảnh ngôn ngữ; nếu muốn dịch, phải đọc `language` từ cài đặt — ghi lại là việc của Phase sau, không mở rộng phạm vi ở đây.

- [ ] **Step 5: Biên dịch và kiểm chứng thủ công**

Run: `cd src-tauri && cargo build`
Expected: biên dịch sạch.

Chạy `pnpm tauri dev`, bắt đầu một tác vụ, thu nhỏ cửa sổ, đợi tác vụ xong — phải nhận được thông báo hệ thống. Lặp lại với cửa sổ đang mở và có focus — không được có thông báo nào.

- [ ] **Step 6: Commit**

```bash
git add src-tauri package.json pnpm-lock.yaml
git commit -m "feat(notifications): notify on job completion when the window is hidden"
```

---

### Task 24: Khay hệ thống và chế độ chạy nền

**Files:**
- Modify: `src-tauri/Cargo.toml` (feature `tray-icon`)
- Create: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Bật feature khay hệ thống**

Trong `src-tauri/Cargo.toml`:

```toml
tauri = { version = "2", features = ["tray-icon"] }
```

- [ ] **Step 2: Viết module khay hệ thống**

Tạo `src-tauri/src/tray.rs`:

```rust
//! Biểu tượng khay hệ thống và hành vi đóng cửa sổ (FR-127, FR-129).

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::db::Db;
use crate::downloader::queue::DownloadQueue;

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open Media Downloader", true, None::<&str>)?;
    let pause_all = MenuItem::with_id(app, "pause_all", "Pause all downloads", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &pause_all, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("app has an icon").clone())
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "pause_all" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(queue) = app.try_state::<DownloadQueue>() {
                        let _ = queue.pause_all().await;
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Đóng cửa sổ: thu về khay nếu người dùng đã bật chạy nền, ngược lại thoát
/// hẳn như hành vi cũ (FR-129).
///
/// Đọc cài đặt tại thời điểm đóng chứ không cache lúc khởi động, để người dùng
/// vừa đổi trong Cài đặt là có hiệu lực ngay.
pub fn should_hide_instead_of_exit(db: &Arc<Db>) -> bool {
    db.get_settings()
        .map(|settings| settings.run_in_background)
        .unwrap_or(false)
}
```

- [ ] **Step 3: Gắn vào vòng đời ứng dụng**

Trong `src-tauri/src/lib.rs`, trong `setup`:

```rust
            tray::build_tray(app.handle())?;
```

Và thêm xử lý sự kiện cửa sổ trên builder:

```rust
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if let Some(db) = app.try_state::<std::sync::Arc<Db>>() {
                    if tray::should_hide_instead_of_exit(&db) {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
```

Khai báo `mod tray;` ở đầu `lib.rs`.

- [ ] **Step 4: Biên dịch**

Run: `cd src-tauri && cargo build`
Expected: biên dịch sạch.

- [ ] **Step 5: Kiểm chứng thủ công**

1. Với chạy nền **tắt** (mặc định): đóng cửa sổ → ứng dụng thoát hẳn, không còn biểu tượng khay.
2. Bật chạy nền, bắt đầu một tác vụ dài, đóng cửa sổ → cửa sổ biến mất, biểu tượng khay còn, tác vụ vẫn tải xong và bắn thông báo.
3. Bấm biểu tượng khay → mở lại cửa sổ với đúng trạng thái hàng đợi.
4. Menu khay → "Pause all downloads" → mở lại cửa sổ, mọi tác vụ ở trạng thái tạm dừng.

- [ ] **Step 6: Commit**

```bash
git add src-tauri
git commit -m "feat(tray): keep downloads running in the system tray"
```

---

### Task 25: Link tải trực tiếp và luồng HLS

**Files:**
- Modify: `src-tauri/src/commands/media.rs:177-198` (thứ tự thử các engine và thông báo lỗi cuối)
- Modify: `src/locales/*.json`

yt-dlp đã xử lý được cả link file trực tiếp lẫn `.m3u8` qua extractor generic của nó. Việc cần làm không phải thêm engine mới, mà là **đảm bảo generic không bị chặn trước** và thông báo lỗi cuối cùng nói rõ đã thử những gì (FR-130, FR-131).

- [ ] **Step 1: Viết test thất bại**

Thêm vào `mod tests` trong `src-tauri/src/commands/media.rs`:

```rust
    #[test]
    fn direct_media_urls_get_a_title_from_the_filename() {
        // yt-dlp trả về metadata rất nghèo cho link file trực tiếp: thường chỉ
        // có `_type: video` và không có `title`. Rơi về "Untitled" cho toàn bộ
        // nhóm này khiến hàng đợi thành một dãy mục không phân biệt được.
        let raw = serde_json::json!({
            "_type": "video",
            "webpage_url": "https://cdn.example.com/clips/holiday-2026.mp4",
        });

        let source = build_media_source(
            "https://cdn.example.com/clips/holiday-2026.mp4",
            "generic",
            &serde_json::from_value(raw).unwrap(),
        );

        assert_eq!(source.title, "holiday-2026.mp4");
    }

    #[test]
    fn unrecognised_urls_report_every_engine_that_was_tried() {
        let error = unsupported_after_all_engines("https://example.com/nope");

        assert_eq!(error.code, "UNSUPPORTED_ALL_ENGINES");
        assert!(error.message.contains("yt-dlp"));
        assert!(error.message.contains("gallery-dl"));
    }
```

- [ ] **Step 2: Chạy test để xác nhận nó thất bại**

Run: `cd src-tauri && cargo test --lib commands::media`
Expected: FAIL — `cannot find function unsupported_after_all_engines`.

- [ ] **Step 3: Hiện thực**

Trong `src-tauri/src/commands/media.rs`, trong `build_media_source`, đổi bước lấy tiêu đề:

```rust
    let title = raw
        .title
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| filename_from_url(source_url));
```

Và thêm hai hàm:

```rust
/// Tên file suy ra từ URL, dùng khi nguồn không cung cấp tiêu đề — điển hình
/// là link trỏ thẳng tới một file media (FR-130).
fn filename_from_url(source_url: &str) -> String {
    url::Url::parse(source_url)
        .ok()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|segments| segments.last().map(str::to_string))
        })
        .filter(|segment| !segment.is_empty())
        .unwrap_or_else(|| "Untitled".to_string())
}

/// Lỗi cuối cùng khi mọi engine đều bó tay. Liệt kê tên engine đã thử để người
/// dùng biết vấn đề nằm ở link chứ không phải ở một cấu hình nào họ quên bật
/// (FR-131).
fn unsupported_after_all_engines(source_url: &str) -> AppError {
    AppError::new(
        "UNSUPPORTED_ALL_ENGINES",
        format!("No engine could read {source_url}. Tried: yt-dlp (including its generic extractor for direct file and HLS links), then gallery-dl."),
    )
}
```

Ở nhánh mà cả hai engine đều thất bại (`media.rs:193`), trả `unsupported_after_all_engines(&source_url)` thay cho lỗi hiện tại.

- [ ] **Step 4: Thêm chuỗi dịch**

`en.json`, mục `errors`:

```json
    "UNSUPPORTED_ALL_ENGINES": "No download engine could read this link. It may not be a media page, or the site may have changed."
```

`vi.json`, mục `errors`:

```json
    "UNSUPPORTED_ALL_ENGINES": "Không engine nào đọc được link này. Có thể đây không phải trang media, hoặc trang nguồn đã thay đổi."
```

- [ ] **Step 5: Chạy test**

Run: `cd src-tauri && cargo test && cd .. && pnpm test`
Expected: PASS.

- [ ] **Step 6: Kiểm chứng thủ công**

Dán một link trỏ thẳng tới file `.mp4` và một link `.m3u8`, xác nhận cả hai xem trước và tải được. Dán một link trang chủ báo bất kỳ, xác nhận thông báo lỗi nêu rõ đã thử yt-dlp rồi gallery-dl.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/media.rs src/locales
git commit -m "feat(preview): support direct file and HLS links with a clearer failure message"
```

---

## Xác minh cuối cùng trước khi coi Phase 1 là xong

Chạy đủ bộ, không bỏ bước nào:

- [ ] `cd src-tauri && cargo test` — mọi test Rust đạt
- [ ] `cd src-tauri && cargo clippy -- -D warnings` — không còn cảnh báo
- [ ] `pnpm test` — mọi test frontend đạt
- [ ] `pnpm exec tsc --noEmit -p tsconfig.json` — không lỗi kiểu (kể cả trong `tests/`)
- [ ] `pnpm lint` — sạch
- [ ] `pnpm tauri build` — dựng được bản cài đặt

Kiểm chứng thủ công theo tiêu chí thành công của spec:

- [ ] **SC-101**: dán 20 link, mọi tác vụ được tạo trong dưới 15 giây, giao diện không đơ
- [ ] **SC-102**: chọn "video" cho lô, xác nhận cả 20 tác vụ là video chứ không phải audio
- [ ] **SC-103**: đổi số luồng lúc đang chạy, có hiệu lực trong dưới 2 giây
- [ ] **SC-104**: buộc đóng ứng dụng lúc đang tải, mở lại, không tác vụ nào kẹt trạng thái
- [ ] **SC-105**: tiếp tục một tác vụ tạm dừng, xác nhận tải nối tiếp chứ không về 0%
- [ ] **SC-106**: dán link video riêng tư, thất bại trong dưới 5 giây
- [ ] **SC-107**: rút mạng 30 giây giữa chừng rồi cắm lại, tác vụ tự hoàn tất
- [ ] **SC-108**: kéo tác vụ cuối lên đầu, nó là tác vụ chạy tiếp theo
- [ ] **SC-109**: đặt giới hạn 500 KB/s, đo tốc độ trong 30 giây, không vượt 110%
- [ ] **SC-110**: `grep -rn "clipboard" src/ src-tauri/src/` chỉ ra kết quả trong trình xử lý sự kiện dán, không có chỗ nào đọc clipboard theo chu kỳ
- [ ] **SC-111**: thêm tạm một key chỉ ở `en.json`, xác nhận `pnpm test` thất bại; xoá đi, xác nhận đạt lại

---

## Ghi chú tự rà soát

Đã đối chiếu plan này với `specs/002-download-power/spec.md`:

| Yêu cầu | Task |
|---|---|
| FR-101 → FR-103 (lô có lựa chọn, xem trước song song, trạng thái từng URL) | 18 |
| FR-104 → FR-107 (kéo-thả, file `.txt`, chọn file, bỏ trùng) | 9, 12, 19 |
| FR-110 (cấm đọc clipboard nền) | Không cần code — được bảo vệ bằng kiểm chứng ở phần xác minh cuối |
| FR-111 → FR-119 (hàng đợi chờ, số luồng, khôi phục, sắp xếp lại, thao tác hàng loạt) | 1, 2, 5, 7, 15, 16, 17, 21 |
| FR-120 → FR-125 (phân loại lỗi, backoff, đếm ngược, huỷ mọi giai đoạn, tranh chấp handle) | 3, 5, 8, 16 |
| FR-126 → FR-129 (giới hạn tốc độ, khay hệ thống, thông báo) | 6, 21, 23, 24 |
| FR-130 → FR-131 (link trực tiếp, HLS, thông báo lỗi) | 25 |
| FR-132 → FR-135 (i18n, parity, lưới gallery, kiểm tra kiểu test) | 10, 13, 20, 22 |

Không có yêu cầu nào trong spec chưa có task tương ứng.

**Hai điểm cần chú ý khi thực thi:**

1. **Task 5 là điểm rủi ro nhất.** Nó thay lõi của `queue.rs` (1299 dòng). Nếu nó vượt tầm kiểm soát, dừng lại và chia nhỏ: (a) đổi struct + dispatcher nhưng vẫn giữ vòng `for attempt` cũ, (b) mới bỏ vòng lặp đó ở một commit riêng. Đừng cố làm cả hai trong một lần.

2. **Thứ tự Nhóm B trước Nhóm C là bắt buộc, không phải gợi ý.** `DownloadForm.tsx` đang 823 dòng; thêm chế độ lô mới, kéo-thả, và lưới gallery vào đó trước khi tách sẽ đẩy nó lên hơn 1000 dòng và làm mọi task sau đó khó rà soát hơn hẳn.
