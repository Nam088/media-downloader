import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

import i18n from "@/lib/i18n";
import { totalPagesOf } from "@/lib/pagination";
import type { AppError, DownloadJob, MediaType } from "@/types/download";
import type {
  LibraryItem,
  LibraryQuery,
  LibraryReconcileReport,
  LibrarySort,
  LibraryStats,
  SortDirection,
} from "@/types/library";

/**
 * Thư viện media đã tải (`specs/004-library`) — trạng thái duyệt phía giao diện.
 *
 * FR-310 quyết định hình dạng của store này. Một thư viện 10.000 mục không được
 * đi qua IPC trong một nhịp, nên mọi lần nạp đều là **một trang** (`limit` +
 * `offset`) — và người dùng chọn trang bằng số, giống hệt trang Lịch sử. Đổi bộ
 * lọc, từ khoá, thứ tự sắp xếp hay cỡ trang thì quay về trang 1 — vì `offset`
 * chỉ có nghĩa trong đúng một truy vấn.
 *
 * Ba thứ được nhớ giữa các phiên (FR-306, FR-309): kiểu hiển thị, tiêu chí sắp
 * xếp và chiều sắp xếp. Chúng nằm ở `localStorage` chứ không phải trong CSDL vì
 * đó là sở thích của một cửa sổ, không phải dữ liệu người dùng — và mất chúng
 * không mất gì cả.
 */

/** Cỡ trang cho người dùng chọn — cùng bộ với trang Lịch sử, vì hai danh sách
 * giờ là cùng một thao tác. */
export const LIBRARY_PAGE_SIZES = [10, 20, 50] as const;

/** Mặc định 20: thư viện thật của người dùng là 66 mục, tức bốn trang — đủ
 * nhiều để thanh trang có việc để làm, đủ ít để không phải cuộn tìm nó. */
export const DEFAULT_LIBRARY_PAGE_SIZE = 20;

/**
 * FR-307 nói kết quả cập nhật "trong lúc người dùng gõ", không nói mỗi phím là
 * một truy vấn. Mỗi lần gọi là một vòng IPC cộng một lượt quét `LIKE '%…%'`
 * trên toàn bảng, nên gõ "podcast" mà bắn bảy truy vấn thì sáu trong số đó là
 * kết quả bị vứt đi. Chờ người dùng ngừng gõ một nhịp rồi mới hỏi.
 */
export const SEARCH_DEBOUNCE_MS = 300;

export type LibraryViewMode = "grid" | "list";

/** Bộ lọc đang áp (FR-308). Các trường khác nhau kết hợp theo "và"; nhiều giá
 * trị trong cùng một trường kết hợp theo "hoặc" — chính là điều `LibraryQuery`
 * mô tả, nên hình dạng ở đây bám sát nó. */
export interface LibraryFilterState {
  search: string;
  media_types: MediaType[];
  platforms: string[];
  formats: string[];
  /** `YYYY-MM-DD` như `<input type="date">` trả về. */
  downloaded_from: string | null;
  downloaded_to: string | null;
  /** FR-324: màn hình dọn dẹp chỉ hiện các mục đang thiếu. */
  missing_only: boolean;
}

const EMPTY_FILTERS: LibraryFilterState = {
  search: "",
  media_types: [],
  platforms: [],
  formats: [],
  downloaded_from: null,
  downloaded_to: null,
  missing_only: false,
};

/** Có bộ lọc nào đang áp không — thứ phân biệt "chưa tải gì" với "không có kết
 * quả cho bộ lọc này" (FR-311). Hai trạng thái ấy trông giống nhau trên màn
 * hình (danh sách rỗng) nhưng cần hai câu chữ và hai hành động khác hẳn nhau. */
export function hasActiveFilters(filters: LibraryFilterState): boolean {
  return (
    filters.search.trim() !== "" ||
    filters.media_types.length > 0 ||
    filters.platforms.length > 0 ||
    filters.formats.length > 0 ||
    filters.downloaded_from !== null ||
    filters.downloaded_to !== null ||
    filters.missing_only
  );
}

/** `<input type="date">` cho một ngày, còn `completed_at` là một mốc thời điểm.
 * Đầu dưới lấy nửa đêm, đầu trên lấy cuối ngày — nếu không, chọn "tới hôm nay"
 * sẽ loại đúng những thứ tải hôm nay. */
function startOfDay(date: string | null): string | null {
  return date === null || date === "" ? null : `${date}T00:00:00Z`;
}

function endOfDay(date: string | null): string | null {
  return date === null || date === "" ? null : `${date}T23:59:59Z`;
}

export function toLibraryQuery(
  filters: LibraryFilterState,
  sort: LibrarySort,
  direction: SortDirection,
  page?: { limit: number; offset: number },
): LibraryQuery {
  return {
    search: filters.search.trim() === "" ? null : filters.search.trim(),
    media_types: filters.media_types,
    platforms: filters.platforms,
    formats: filters.formats,
    downloaded_from: startOfDay(filters.downloaded_from),
    downloaded_to: endOfDay(filters.downloaded_to),
    is_missing: filters.missing_only ? true : null,
    sort,
    direction,
    limit: page?.limit ?? null,
    offset: page?.offset ?? null,
  };
}

const VIEW_MODE_KEY = "library.view_mode";
const SORT_KEY = "library.sort";
const DIRECTION_KEY = "library.direction";

const SORTS: LibrarySort[] = ["downloaded_at", "title", "file_size", "duration"];

function readStored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const stored = localStorage.getItem(key);
    return stored !== null && (allowed as readonly string[]).includes(stored)
      ? (stored as T)
      : fallback;
  } catch {
    return fallback;
  }
}

function writeStored(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Ghi hỏng (chế độ riêng tư, hết hạn ngạch) chỉ có nghĩa là lần mở sau
    // quay về mặc định — không đáng để làm hỏng thao tác người dùng vừa làm.
  }
}

/** Lỗi từ `invoke` đã là `{ code, message }` do `AppError` tuần tự hoá; mọi thứ
 * khác (lỗi JS, chuỗi trần) được gói lại để dải hiển thị lỗi luôn có mã. */
export function toAppError(error: unknown): AppError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof (error as AppError).code === "string"
  ) {
    return error as AppError;
  }
  return { code: "INTERNAL", message: String(error) };
}

interface LibraryState {
  items: LibraryItem[];
  stats: LibraryStats | null;
  /** Ảnh chụp thống kê KHÔNG áp bộ lọc, dùng để dựng danh sách lựa chọn của
   * chính các bộ lọc. Lấy `stats` làm việc này thì sau khi chọn "youtube",
   * danh sách nền tảng chỉ còn mỗi YouTube và người dùng không thêm được
   * "tiktok" vào lựa chọn nữa — một bộ lọc tự khoá mình lại. */
  facets: LibraryStats | null;
  /** Những gì đang ở trong ô tìm kiếm ngay lúc này — cập nhật mỗi phím gõ. */
  searchInput: string;
  /** Bộ lọc đã thật sự gửi xuống backend; `search` ở đây trễ hơn `searchInput`
   * đúng một nhịp debounce. */
  filters: LibraryFilterState;
  sort: LibrarySort;
  direction: SortDirection;
  viewMode: LibraryViewMode;
  selectedIds: string[];
  loading: boolean;
  /** Trang đang xem, đếm từ 1 — cùng con số hiện trên nút. */
  page: number;
  pageSize: number;
  /** Tổng số mục KHỚP BỘ LỌC hiện tại, do chính `library_stats` của lần nạp
   * này trả về. Nó và `items` luôn tả cùng một truy vấn, nên số trang không bao
   * giờ hứa một trang mà `list_library` không có gì để trả. */
  totalItems: number;
  reconciling: boolean;
  error: AppError | null;
  initialized: boolean;

  ensureLoaded: () => Promise<void>;
  /** Nạp lại ĐÚNG trang đang xem (kèm cắt xuống nếu nó không còn tồn tại). */
  reload: () => Promise<void>;
  setPage: (page: number) => Promise<void>;
  setPageSize: (pageSize: number) => Promise<void>;
  setSearch: (value: string) => void;
  setFilters: (patch: Partial<LibraryFilterState>) => void;
  toggleMediaType: (mediaType: MediaType) => void;
  togglePlatform: (platform: string) => void;
  toggleFormat: (format: string) => void;
  clearFilters: () => void;
  setSort: (sort: LibrarySort, direction: SortDirection) => void;
  setViewMode: (mode: LibraryViewMode) => void;
  toggleSelected: (itemId: string) => void;
  selectAllVisible: () => void;
  clearSelection: () => void;
  setError: (error: AppError | null) => void;
  reconcile: () => Promise<void>;
  applyReconciled: (changedItemIds: string[]) => void;
  /** Các mục đang chọn, **theo đúng thứ tự đang hiển thị** — thứ FR-330 đòi
   * cho danh sách phát. Không chọn gì nghĩa là "tất cả những gì đang hiện". */
  selectionInDisplayOrder: () => LibraryItem[];
  renameItem: (itemId: string, newName: string) => Promise<boolean>;
  moveItems: (itemIds: string[], targetDirectory: string) => Promise<boolean>;
  deleteItems: (itemIds: string[]) => Promise<boolean>;
  removeItems: (itemIds: string[]) => Promise<boolean>;
  relinkItem: (itemId: string, newPath: string) => Promise<boolean>;
  redownloadItem: (itemId: string) => Promise<boolean>;
  revealItem: (itemId: string) => Promise<boolean>;
  exportPlaylist: (itemIds: string[], destinationPath: string) => Promise<boolean>;
}

/** Trang đang bay. Một câu trả lời cho bộ lọc CŨ về sau câu trả lời cho bộ lọc
 * mới là chuyện bình thường khi người dùng gõ nhanh; đếm số hiệu yêu cầu để
 * câu trả lời cũ bị bỏ qua thay vì ghi đè lên kết quả đúng. */
let requestSequence = 0;
let searchTimer: ReturnType<typeof setTimeout> | null = null;

export const useLibraryStore = create<LibraryState>((set, get) => {
  /** Chạy một thao tác ghi và biến thất bại thành `error` hiển thị được.
   * Không có nhánh "tự xử lý": `FILE_EXISTS` nghĩa là thao tác đã BỊ TỪ CHỐI
   * (FR-322), nên nó phải nổi lên thành lỗi người dùng phải quyết định, chứ
   * không phải một cảnh báo rồi vẫn ghi đè. */
  async function guard<T>(operation: () => Promise<T>): Promise<T | null> {
    try {
      set({ error: null });
      return await operation();
    } catch (error) {
      set({ error: toAppError(error) });
      return null;
    }
  }

  /** Ảnh chụp KHÔNG áp bộ lọc dùng để dựng chính các bộ lọc. Tách khỏi `stats`
   * một cách cố ý: `stats` tính THÔNG QUA bộ lọc đang áp, nên lấy nó dựng danh
   * sách nền tảng thì chọn "youtube" xong sẽ không còn nền tảng nào khác để
   * chọn nữa — một bộ lọc tự khoá mình lại. */
  async function refreshFacets(stats: LibraryStats): Promise<void> {
    const { filters, sort, direction } = get();
    // Khi chưa có bộ lọc nào, `stats` CHÍNH LÀ tập lựa chọn đầy đủ — nên danh
    // sách nền tảng/định dạng được làm mới miễn phí ở lần nạp đầu và mỗi lần
    // người dùng xoá hết bộ lọc, không tốn thêm vòng IPC nào.
    if (!hasActiveFilters(filters)) {
      set({ facets: stats });
      return;
    }
    // Chỉ chạy khi lần nạp đầu tiên đã có sẵn bộ lọc (khôi phục trạng thái,
    // hoặc bấm vào một dòng thống kê trước khi lưới kịp nạp xong): không có
    // ảnh chụp đầy đủ thì bộ lọc nền tảng sẽ rỗng vĩnh viễn.
    if (get().facets !== null) return;
    try {
      set({
        facets: await invoke<LibraryStats>("library_stats", {
          query: toLibraryQuery(EMPTY_FILTERS, sort, direction),
        }),
      });
    } catch (error) {
      set({ error: toAppError(error) });
    }
  }

  /** Số liệu luôn tính trên đúng bộ lọc đang áp, nên chúng mô tả thứ đang hiển
   * thị chứ không phải toàn thư viện (SC-307). */
  async function refreshStats(): Promise<void> {
    const { filters, sort, direction } = get();
    try {
      const stats = await invoke<LibraryStats>("library_stats", {
        query: toLibraryQuery(filters, sort, direction),
      });
      set({ stats, totalItems: stats.total_items });
      await refreshFacets(stats);
    } catch (error) {
      set({ error: toAppError(error) });
    }
  }

  /**
   * Nạp một trang. Trang VÀ tổng số đi cùng một nhịp `Promise.all`, nên chúng
   * luôn trả lời cùng một câu hỏi: hỏi rời nhau thì một lần xoá xen vào giữa sẽ
   * cho ra "trang 4/3" — một thanh phân trang nói dối về chính danh sách nó
   * đang phân trang.
   */
  async function loadPage(requested: number): Promise<void> {
    const sequence = ++requestSequence;
    const { filters, sort, direction, pageSize } = get();
    const page = Math.max(1, Math.floor(requested));
    set({ loading: true, error: null });

    // Cùng bộ lọc, cùng thứ tự; chỉ khác đúng `limit`/`offset` — thứ mà
    // `library_stats` không cần và không được nhận, vì nó đếm cả tập.
    const pageQuery = toLibraryQuery(filters, sort, direction, {
      limit: pageSize,
      offset: (page - 1) * pageSize,
    });
    const countQuery = toLibraryQuery(filters, sort, direction);

    let items: LibraryItem[];
    let stats: LibraryStats;
    try {
      [items, stats] = await Promise.all([
        invoke<LibraryItem[]>("list_library", { query: pageQuery }),
        invoke<LibraryStats>("library_stats", { query: countQuery }),
      ]);
    } catch (error) {
      // Câu trả lời của một truy vấn đã bị bỏ không được phép dựng lại lỗi
      // của nó lên màn hình đang hiển thị kết quả khác.
      if (sequence === requestSequence) set({ loading: false, error: toAppError(error) });
      return;
    }
    if (sequence !== requestSequence) return;

    // Danh sách teo lại dưới chân người dùng (vừa xoá, hoặc bộ lọc của người
    // khác vừa cắt bớt): rơi xuống trang cuối CÒN TỒN TẠI thay vì hiện một
    // trang rỗng và bắt họ tự bấm ngược lại. Đệ quy có đáy vì `pages < page`.
    const pages = totalPagesOf(stats.total_items, pageSize);
    if (page > pages) {
      await loadPage(pages);
      return;
    }

    set({
      items,
      stats,
      totalItems: stats.total_items,
      page,
      loading: false,
      // Ô tích thuộc về những dòng đang nhìn thấy: giữ lại id của trang trước
      // thì thanh công cụ vẫn nói "đã chọn 3" trong khi không dòng nào trên
      // màn hình được tích, và "xoá mục đã chọn" sẽ xoá thứ người dùng không
      // còn thấy.
      selectedIds: [],
    });
    await refreshFacets(stats);
  }

  /**
   * Thay tại chỗ những mục vừa đổi, thay vì nạp lại cả trang.
   *
   * Đổi tên và di chuyển KHÔNG làm thay đổi số mục, nên số trang vẫn đúng và
   * cái giá duy nhất của việc vá tại chỗ là: nếu đang sắp theo tiêu đề, dòng
   * vừa đổi tên có thể lẽ ra phải nằm ở trang khác. Nó vẫn ở lại đây tới lần
   * nạp sau — và đó là lựa chọn cố ý: bắn dòng người dùng vừa sửa sang một
   * trang khác ngay giữa thao tác trông y hệt như vừa xoá nhầm nó, còn cái tên
   * mới hiện ngay tại chỗ chính là câu xác nhận "đã đổi xong".
   */
  function patchItems(updated: LibraryItem[]): void {
    if (updated.length === 0) return;
    const byId = new Map(updated.filter((item) => item != null).map((item) => [item.id, item]));
    set((state) => ({
      items: state.items.map((item) => byId.get(item.id) ?? item),
    }));
  }

  function dropItems(itemIds: string[]): void {
    const removed = new Set(itemIds);
    set((state) => ({
      items: state.items.filter((item) => !removed.has(item.id)),
      selectedIds: state.selectedIds.filter((id) => !removed.has(id)),
    }));
  }

  return {
    items: [],
    stats: null,
    facets: null,
    searchInput: "",
    filters: EMPTY_FILTERS,
    sort: readStored<LibrarySort>(SORT_KEY, SORTS, "downloaded_at"),
    direction: readStored<SortDirection>(DIRECTION_KEY, ["asc", "desc"], "desc"),
    viewMode: readStored<LibraryViewMode>(VIEW_MODE_KEY, ["grid", "list"], "grid"),
    selectedIds: [],
    loading: false,
    page: 1,
    pageSize: DEFAULT_LIBRARY_PAGE_SIZE,
    totalItems: 0,
    reconciling: false,
    error: null,
    initialized: false,

    // `App.tsx` dựng mọi trang cùng lúc và chỉ ẩn/hiện bằng class, nên trang
    // Thư viện tồn tại ngay cả khi người dùng chưa từng bấm vào tab của nó.
    // Nạp theo `useEffect` trần ở đó nghĩa là mọi lần khởi động ứng dụng đều
    // trả tiền cho một truy vấn thư viện mà chưa ai nhìn.
    ensureLoaded: async () => {
      if (get().initialized) return;
      set({ initialized: true });
      await get().reload();
    },

    reload: async () => {
      await loadPage(get().page);
    },

    setPage: async (page) => {
      await loadPage(page);
    },

    // Đổi cỡ trang là đổi câu hỏi: "trang 3" của cỡ cũ không trỏ tới cùng
    // những dòng ấy ở cỡ mới, nên quay về trang 1 thay vì giả vờ nó còn nghĩa.
    setPageSize: async (pageSize) => {
      set({ pageSize });
      await loadPage(1);
    },

    setSearch: (value) => {
      set({ searchInput: value });
      if (searchTimer !== null) clearTimeout(searchTimer);
      searchTimer = setTimeout(() => {
        searchTimer = null;
        const { searchInput, filters } = get();
        if (filters.search === searchInput) return;
        set({ filters: { ...filters, search: searchInput } });
        // Câu hỏi đã khác — "trang 3 của câu hỏi trước" không tả gì cả.
        void loadPage(1);
      }, SEARCH_DEBOUNCE_MS);
    },

    setFilters: (patch) => {
      set((state) => ({ filters: { ...state.filters, ...patch } }));
      void loadPage(1);
    },

    toggleMediaType: (mediaType) => {
      const current = get().filters.media_types;
      get().setFilters({
        media_types: current.includes(mediaType)
          ? current.filter((value) => value !== mediaType)
          : [...current, mediaType],
      });
    },

    togglePlatform: (platform) => {
      const current = get().filters.platforms;
      get().setFilters({
        platforms: current.includes(platform)
          ? current.filter((value) => value !== platform)
          : [...current, platform],
      });
    },

    toggleFormat: (format) => {
      const current = get().filters.formats;
      get().setFilters({
        formats: current.includes(format)
          ? current.filter((value) => value !== format)
          : [...current, format],
      });
    },

    clearFilters: () => {
      if (searchTimer !== null) {
        clearTimeout(searchTimer);
        searchTimer = null;
      }
      set({ filters: EMPTY_FILTERS, searchInput: "" });
      void loadPage(1);
    },

    setSort: (sort, direction) => {
      writeStored(SORT_KEY, sort);
      writeStored(DIRECTION_KEY, direction);
      set({ sort, direction });
      // Cùng tập kết quả nhưng khác thứ tự: mục thứ 41 của thứ tự cũ không nằm
      // ở trang 3 của thứ tự mới, nên giữ nguyên số trang là giữ một chỗ ngẫu
      // nhiên trong một danh sách khác.
      void loadPage(1);
    },

    setViewMode: (mode) => {
      writeStored(VIEW_MODE_KEY, mode);
      set({ viewMode: mode });
    },

    toggleSelected: (itemId) =>
      set((state) => ({
        selectedIds: state.selectedIds.includes(itemId)
          ? state.selectedIds.filter((id) => id !== itemId)
          : [...state.selectedIds, itemId],
      })),

    selectAllVisible: () => set((state) => ({ selectedIds: state.items.map((item) => item.id) })),

    clearSelection: () => set({ selectedIds: [] }),

    setError: (error) => set({ error }),

    reconcile: async () => {
      if (get().reconciling) return;
      set({ reconciling: true, error: null });
      try {
        await invoke<LibraryReconcileReport>("reconcile_library", {});
      } catch (error) {
        set({ error: toAppError(error) });
      }
      set({ reconciling: false });
      await refreshStats();
    },

    // Sự kiện chỉ mang danh sách id ĐÃ ĐỔI trạng thái, nên "đổi" ở đây đúng
    // nghĩa là lật cờ: mục đang thiếu vừa quay lại, hoặc mục đang có vừa biến
    // mất. Lật tại chỗ giữ đúng lời hứa "chỉ cập nhật những id đã đổi" mà
    // không cần thêm một vòng IPC nào để hỏi lại backend.
    applyReconciled: (changedItemIds) => {
      if (changedItemIds.length === 0) return;
      const changed = new Set(changedItemIds);
      set((state) => ({
        items: state.items.map((item) =>
          changed.has(item.id) ? { ...item, is_missing: !item.is_missing } : item,
        ),
      }));
    },

    selectionInDisplayOrder: () => {
      const { items, selectedIds } = get();
      if (selectedIds.length === 0) return items;
      const selected = new Set(selectedIds);
      return items.filter((item) => selected.has(item.id));
    },

    renameItem: async (itemId, newName) => {
      const renamed = await guard(() =>
        invoke<LibraryItem>("rename_library_item", { itemId, newName }),
      );
      if (renamed === null) return false;
      patchItems([renamed]);
      await refreshStats();
      return true;
    },

    moveItems: async (itemIds, targetDirectory) => {
      const moved = await guard(() =>
        invoke<LibraryItem[]>("move_library_items", { itemIds, targetDirectory }),
      );
      if (moved === null) return false;
      patchItems(moved);
      await refreshStats();
      return true;
    },

    // Xoá thì KHÁC đổi tên: nó làm tập kết quả co lại, nên trang đang xem vừa
    // thủng một lỗ và tổng số trang có thể đã giảm. Bỏ dòng đi ngay để phản hồi
    // tức thì, rồi nạp lại đúng trang ấy — `loadPage` sẽ kéo các dòng của trang
    // sau lấp vào chỗ trống, và tự cắt xuống nếu trang này không còn tồn tại.
    deleteItems: async (itemIds) => {
      const deleted = await guard(() => invoke<number>("delete_library_items", { itemIds }));
      if (deleted === null) return false;
      dropItems(itemIds);
      await loadPage(get().page);
      return true;
    },

    removeItems: async (itemIds) => {
      const removed = await guard(() => invoke<number>("remove_library_items", { itemIds }));
      if (removed === null) return false;
      dropItems(itemIds);
      await loadPage(get().page);
      return true;
    },

    relinkItem: async (itemId, newPath) => {
      const relinked = await guard(() =>
        invoke<LibraryItem>("relink_library_item", { itemId, newPath }),
      );
      if (relinked === null) return false;
      patchItems([relinked]);
      await refreshStats();
      return true;
    },

    redownloadItem: async (itemId) => {
      const job = await guard(() => invoke<DownloadJob>("redownload_library_item", { itemId }));
      if (job === null) return false;
      toast.success(i18n.t("library.redownload_queued"));
      return true;
    },

    revealItem: async (itemId) => {
      const revealed = await guard(() => invoke<void>("reveal_library_item", { itemId }));
      return revealed !== null;
    },

    exportPlaylist: async (itemIds, destinationPath) => {
      const written = await guard(() =>
        invoke<string>("export_library_playlist", { itemIds, destinationPath }),
      );
      if (written === null) return false;
      toast.success(i18n.t("library.export_done", { path: written }));
      return true;
    },
  };
});
