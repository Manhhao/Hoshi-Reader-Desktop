import { invoke } from "@tauri-apps/api/core";
import { clearCustomShelf, renameCustomShelf } from "./bookSort.svelte";
import type { BookMetadata, BookShelf, ShelfSource } from "./types";

const ORDERS_KEY = "bookshelfShelfOrders";

function storedOrders() {
  try {
    return JSON.parse(localStorage.getItem(ORDERS_KEY) ?? "{}") as Record<string, string[]>;
  } catch {
    return {};
  }
}

export const shelves = $state<{ list: BookShelf[]; orders: Record<string, string[]> }>({
  list: [],
  orders: storedOrders(),
});

export async function loadShelves() {
  shelves.list = await invoke<BookShelf[]>("load_shelves");
}

export function saveShelfOrders() {
  localStorage.setItem(ORDERS_KEY, JSON.stringify($state.snapshot(shelves.orders)));
}

export async function createShelf(name: string) {
  if (shelves.list.some((shelf) => shelf.name === name)) return;
  await invoke("create_shelf", { name });
  await loadShelves();
}

export async function deleteShelf(name: string) {
  clearCustomShelf(name);
  delete shelves.orders[name];
  saveShelfOrders();
  await invoke("delete_shelf", { name });
  await loadShelves();
}

export async function renameShelf(name: string, newName: string) {
  await invoke("rename_shelf", { name, newName });
  renameCustomShelf(name, newName);
  const order = shelves.orders[name];
  delete shelves.orders[name];
  if (order) shelves.orders[newName] = order;
  saveShelfOrders();
  await loadShelves();
}

export function moveShelves() {
  invoke("move_shelves", { names: shelves.list.map((shelf) => shelf.name) });
}

export function setShelfOrder(name: string, bookIds: string[]) {
  shelves.orders[name] = bookIds;
}

export async function moveBook(id: string, name: string | null) {
  await invoke("move_book", { id, name });
  await loadShelves();
}

function isReading(book: BookMetadata) {
  return book.progress > 0 && book.progress < 0.999;
}

function authorKey(name: string) {
  return name.replace(/\s/g, "");
}

export function shelfBooks(books: BookMetadata[], source: ShelfSource) {
  switch (source.kind) {
    case "home":
      return books.filter(isReading);
    case "shelf": {
      const ids = new Set(shelves.list.find((shelf) => shelf.name === source.name)?.bookIds ?? []);
      return books.filter((book) => ids.has(book.id));
    }
    case "unshelved": {
      const ids = new Set(shelves.list.flatMap((shelf) => shelf.bookIds));
      return books.filter((book) => !ids.has(book.id));
    }
    case "author": {
      const key = authorKey(source.name);
      return books.filter((book) => book.author && authorKey(book.author) === key);
    }
    default:
      return books;
  }
}

export function sourceTitle(source: ShelfSource) {
  if ("name" in source) return source.name;
  return source.kind === "unshelved" ? "Unshelved" : "All Books";
}

export function sameSource(a: ShelfSource, b: ShelfSource) {
  if (a.kind !== b.kind) return false;
  if (a.kind === "author" && b.kind === "author") return authorKey(a.name) === authorKey(b.name);
  return !("name" in a) || a.name === (b as { name: string }).name;
}

export function authors(books: BookMetadata[]) {
  const counts = new Map<string, number>();
  for (const book of books) {
    if (book.author) counts.set(book.author, (counts.get(book.author) ?? 0) + 1);
  }
  const groups = new Map<string, { name: string; count: number }>();
  for (const [name, count] of counts) {
    const key = authorKey(name);
    const group = groups.get(key);
    if (group) {
      group.count += count;
      if (count > counts.get(group.name)!) group.name = name;
    } else {
      groups.set(key, { name, count });
    }
  }
  return [...groups.values()]
    .sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }));
}
