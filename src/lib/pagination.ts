/**
 * Numbered pagination — the arithmetic both paged lists need.
 *
 * Lives here rather than next to one of the lists because the History page and
 * the Library page draw the same control and must collapse long runs the same
 * way; two copies of this drift the moment one of them gains a page.
 */

/** A rendered slot in the page bar: either a page to jump to, or the gap that
 * stands in for the pages we chose not to draw. */
export type PageToken = number | "ellipsis";

/** How many pages a result set of `total` rows breaks into.
 *
 * Never zero: an empty result set is still "page 1 of 1", and returning 0 here
 * would make every `page > totalPages` clamp send the user to page 0. */
export function totalPagesOf(total: number, pageSize: number): number {
  if (pageSize <= 0) return 1;
  return Math.max(1, Math.ceil(total / pageSize));
}

/** Page numbers to render, collapsing a long run into `1 … 4 5 6 … 20`
 * instead of one button per page.
 *
 * Up to seven pages every number is drawn — that is the whole library at the
 * sizes this app actually sees (66 files at 20 a page is four buttons), and
 * collapsing four buttons would hide nothing while costing a click. */
export function pageNumbers(current: number, total: number): PageToken[] {
  if (total <= 7) {
    return Array.from({ length: total }, (_unused, index) => index + 1);
  }
  const keep = new Set([1, total, current - 1, current, current + 1]);
  const sorted = [...keep].filter((page) => page >= 1 && page <= total).sort((a, b) => a - b);
  const result: PageToken[] = [];
  let previous = 0;
  for (const page of sorted) {
    if (previous && page - previous > 1) result.push("ellipsis");
    result.push(page);
    previous = page;
  }
  return result;
}
