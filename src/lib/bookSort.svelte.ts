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

const storedOption = localStorage.getItem(KEY) as SortOption | null;
const storedOptions: SortOption[] = ["Title", "Author", "Progress", "Time Read", "Characters"];

const romanValues: Record<string, number> = { I: 1, V: 5, X: 10, L: 50, C: 100 };
const romanPattern = /(?<=[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}ー々] ?)[IVXLC]+(?![A-Za-z])/gu;
const validRoman = /^C{0,3}(?:XC|XL|L?X{0,3})(?:IX|IV|V?I{0,3})$/;
const parts: Record<string, string> = { 上: "1", 中: "2", 下: "3", 前: "1", 後: "2" };

function romanToNumber(roman: string) {
  let total = 0;
  for (let i = 0; i < roman.length; i++) {
    const value = romanValues[roman[i]];
    total += value < (romanValues[roman[i + 1]] ?? 0) ? -value : value;
  }
  return total;
}

export function titleKey(title: string) {
  return title
    .normalize("NFKC")
    .replace(romanPattern, (roman) => (validRoman.test(roman) ? String(romanToNumber(roman)) : roman))
    .replace(/\(([上中下前後])\)/g, (_, part: string) => `(${parts[part]})`)
    .replace(/(?<=\s)[上中下](?=\s|$)/g, (part) => parts[part]);
}

export const bookSort = $state<{ option: SortOption; reversed: boolean; customShelves: string[] }>({
  option: storedOption && storedOptions.includes(storedOption) ? storedOption : "Recent",
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
    const position = new Map(order.map((folder, index) => [folder, index]));
    const rank = (book: BookMetadata) => position.get(book.folder) ?? order.length;
    return books.toSorted((a, b) => rank(a) - rank(b));
  }
  const keys =
    option === "Title" || option === "Author"
      ? new Map(books.map((book) => [book.id, titleKey(book.renamedTitle ?? book.title)]))
      : null;
  const byTitle = (a: BookMetadata, b: BookMetadata) =>
    keys!.get(a.id)!.localeCompare(keys!.get(b.id)!, undefined, {
      numeric: true,
      sensitivity: "base",
    });
  const sorted = books.toSorted((a, b) => {
    switch (option) {
      case "Title":
        return byTitle(a, b);
      case "Author":
        return (
          Number(!a.author) - Number(!b.author) ||
          (a.author ?? "").localeCompare(b.author ?? "", undefined, { sensitivity: "base" }) ||
          byTitle(a, b)
        );
      case "Progress":
        return b.progress - a.progress;
      case "Time Read":
        return (readingTimes[b.id] ?? 0) - (readingTimes[a.id] ?? 0);
      case "Characters":
        return b.charactersTotal - a.charactersTotal;
      default:
        return b.lastAccess - a.lastAccess;
    }
  });
  return reversed ? sorted.reverse() : sorted;
}
