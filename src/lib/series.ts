import type { BookMetadata } from "./types";

export type Series = { key: string; name: string; books: BookMetadata[] };

const noise = [/【電子[^】]*】/g, /[(（][^()（）]*(?:文庫|ブックス)[)）]/g, /BOOK☆WALKER special edition/gi];

function cleanTitle(book: BookMetadata) {
  let title = (book.renamedTitle ?? book.title).normalize("NFKC");
  for (const pattern of noise) title = title.replace(pattern, " ");
  return title.replace(/\s+/g, " ").trim();
}

function comparable(title: string) {
  return title.replace(/[\s()[\]「」『』〈〉《》【】]/g, "");
}

function authorKey(author: string | null | undefined) {
  return (author ?? "").normalize("NFKC").replace(/\s/g, "");
}

function nameKey(name: string) {
  return name.normalize("NFKC").replace(/\s/g, "").toLowerCase();
}

function commonPrefix(a: string, b: string) {
  let i = 0;
  while (i < a.length && a[i] === b[i]) i++;
  return a.slice(0, i);
}

function seriesName(prefix: string) {
  let name = prefix;
  for (let previous = ""; previous !== name; ) {
    previous = name;
    name = name.replace(/(?:[\s\d.#([\-–—~～・:：,、]|vol\.?|第)+$/i, "");
  }
  return name;
}

export function series(books: BookMetadata[], names: Record<string, string> = {}): Series[] {
  const byAuthor = new Map<string, { book: BookMetadata; title: string; key: string }[]>();
  const manual: BookMetadata[] = [];
  for (const book of books) {
    if (book.series?.trim()) {
      manual.push(book);
      continue;
    }
    const title = cleanTitle(book);
    const author = authorKey(book.author);
    if (!byAuthor.has(author)) byAuthor.set(author, []);
    byAuthor.get(author)!.push({ book, title, key: comparable(title) });
  }

  const result: Series[] = [];
  for (const [author, entries] of byAuthor) {
    entries.sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
    let group: typeof entries = [];
    const flush = () => {
      if (new Set(group.map((entry) => entry.key)).size > 1) {
        const key = `${author}\u0000${group[0].key.slice(0, 3)}`;
        const prefix = group.map((entry) => entry.title).reduce(commonPrefix);
        const name = seriesName(prefix) || seriesName(group.map((entry) => entry.key).reduce(commonPrefix));
        result.push({ key, name: names[key] || name, books: group.map((entry) => entry.book) });
      }
      group = [];
    };
    for (const entry of entries) {
      if (group.length > 0 && commonPrefix(group[0].key, entry.key).length < 3) flush();
      group.push(entry);
    }
    flush();
  }

  for (const book of manual) {
    const name = book.series!.trim();
    const existing = result.find((group) => nameKey(group.name) === nameKey(name));
    if (existing) {
      existing.books.push(book);
    } else {
      const key = `manual\u0000${nameKey(name)}`;
      result.push({ key, name: names[key] || name, books: [book] });
    }
  }

  return result.sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }));
}
