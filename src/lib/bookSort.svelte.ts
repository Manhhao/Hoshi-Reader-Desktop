import type { BookMetadata, SortOption } from "./types";

const KEY = "bookshelfSortOption";
const CUSTOM_KEY = "bookshelfCustomShelves";
const REVERSED_KEY = "bookshelfSortReversed";

function storedCustomShelves() {
  try {
    return JSON.parse(localStorage.getItem(CUSTOM_KEY) ?? "[]") as string[];
  } catch {
    return [];
  }
}

const storedOption = localStorage.getItem(KEY);

export const bookSort = $state<{ option: SortOption; reversed: boolean; customShelves: string[] }>({
  option: storedOption === "Title" || storedOption === "Progress" || storedOption === "Time Read" ? storedOption : "Recent",
  reversed: localStorage.getItem(REVERSED_KEY) === "true",
  customShelves: storedCustomShelves(),
});

function saveCustomShelves() {
  localStorage.setItem(CUSTOM_KEY, JSON.stringify(bookSort.customShelves));
}

export function sortOption(shelf: string | null): SortOption {
  return shelf !== null && bookSort.customShelves.includes(shelf) ? "Custom" : bookSort.option;
}

export function setBookSort(option: SortOption, shelf: string | null) {
  if (option === "Custom") {
    if (shelf === null || bookSort.customShelves.includes(shelf)) return;
    bookSort.customShelves = [...bookSort.customShelves, shelf];
    saveCustomShelves();
    return;
  }
  if (shelf !== null) {
    clearCustomShelf(shelf);
  }
  bookSort.option = option;
  bookSort.reversed = false;
  localStorage.setItem(KEY, option);
  localStorage.setItem(REVERSED_KEY, "false");
}

export function reverseBookSort() {
  bookSort.reversed = !bookSort.reversed;
  localStorage.setItem(REVERSED_KEY, String(bookSort.reversed));
}

export function clearCustomShelf(shelf: string) {
  bookSort.customShelves = bookSort.customShelves.filter((name) => name !== shelf);
  saveCustomShelves();
}

export function renameCustomShelf(shelf: string, newName: string) {
  bookSort.customShelves = bookSort.customShelves.map((name) => (name === shelf ? newName : name));
  saveCustomShelves();
}

export function sortBooks(
  books: BookMetadata[],
  option: SortOption,
  order: string[],
  reversed = false,
  readingTimes: Record<string, number> = {},
) {
  if (option === "Custom") {
    const position = new Map(order.map((id, index) => [id, index]));
    const rank = (book: BookMetadata) => position.get(book.id) ?? order.length;
    return books.toSorted((a, b) => rank(a) - rank(b));
  }
  const sorted = books.toSorted((a, b) => {
    switch (option) {
      case "Title":
        return (a.renamedTitle ?? a.title).localeCompare(b.renamedTitle ?? b.title, undefined, {
          numeric: true,
          sensitivity: "base",
        });
      case "Progress":
        return b.progress - a.progress;
      case "Time Read":
        return (readingTimes[b.id] ?? 0) - (readingTimes[a.id] ?? 0);
      default:
        return b.lastAccess - a.lastAccess;
    }
  });
  return reversed ? sorted.reverse() : sorted;
}
