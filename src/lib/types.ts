export type BookMetadata = {
  id: string;
  title: string;
  author?: string | null;
  epub?: string | null;
  cover?: string | null;
  folder: string;
  lastAccess: number;
  renamedTitle?: string | null;
  characterCount?: number | null;
  progress: number;
  charactersRead: number;
  charactersTotal: number;
  hasBookInfo: boolean;
};

export type Timestamped<T> = {
  modified: number;
  value: T;
};

export type SortOption = "Recent" | "Title" | "Progress" | "Time Read" | "Custom";

export type SelectionRect = { x: number; y: number; width: number; height: number };

export type AppView = "books" | "dictionary" | "statistics" | "settings";

export type BookShelf = {
  name: string;
  bookIds: string[];
};

export type ShelfSource =
  | { kind: "all" | "home" }
  | { kind: "shelf" | "author"; name: string };

export type ChapterInfo = {
  spineIndex: number | null;
  currentTotal: number;
  chapterCount: number;
  fragmentOffsets?: Record<string, number> | null;
};

export type BookInfo = {
  characterCount: number;
  chapterInfo: Record<string, ChapterInfo>;
  images?: string[] | null;
};

export type Bookmark = {
  chapterIndex: number;
  progress: number;
  characterCount: number;
};

export type ReadingSession = {
  startedAt: number;
  endedAt: number;
  charactersRead: number;
  readingTime: number;
};

export type Sessions = Record<string, Timestamped<ReadingSession | null>>;

export type ReadingDay = {
  dateKey: string;
  charactersRead: number;
  readingTime: number;
};

export type BookStatistics = {
  id: string;
  folder: string;
  title: string;
  cover: boolean;
  isDeleted: boolean;
  days: ReadingDay[];
};

export type StatisticsPeriod = "Week" | "Month" | "Year" | "All";

export type SasayakiMatch = {
  id: string;
  startTime: number;
  endTime: number;
  text: string;
  chapterIndex: number;
  start: number;
  length: number;
};

export type SasayakiImage = {
  chapterIndex: number;
  imageIndex: number;
  offset: number;
};

export type SasayakiMatchData = {
  matches: SasayakiMatch[];
  unmatched: number;
  images: SasayakiImage[];
};

export type SasayakiPlayback = {
  lastPosition: number;
  delay: number;
  rate: number;
  volume: number;
  audioPath?: string | null;
};

export type SyncQueueItem = {
  key: string;
  title: string;
  direction: "upload" | "download" | "both" | null;
  error: string | null;
};

export type GoogleDriveSyncStatus = {
  lastSync: number | null;
  isSyncing: boolean;
  errorMessage: string | null;
  queue: SyncQueueItem[];
  progress: { done: number; total: number; current: string | null } | null;
};

export type SyncBook = {
  bookmark?: Timestamped<{ characterCount: number }>;
  audiobook?: Timestamped<{ lastPosition: number; delay: number; rate: number }>;
  sessions: Sessions;
};

export type TocItem = {
  label: string;
  spineIndex: number;
  fragment: string | null;
  indentLevel: number;
};

export type BookDocument = {
  title: string;
  spine: string[];
  toc: TocItem[];
};

export type LookupEntry = Record<string, unknown> & { matched: string };

export type LookupResponse = {
  entries: LookupEntry[];
  styles: Record<string, string>;
};

export type KanjiEntry = {
  dictName: string;
  onyomi: string;
  kunyomi: string;
  meanings: string[];
};

export type KanjiResponse = {
  character: string;
  entries: KanjiEntry[];
};

export type PopupAnkiConfig = {
  useAnkiConnect: boolean;
  isAnkiConnectReachable: boolean;
  allowDupes: boolean;
  disableShowNotes: boolean;
  compactGlossariesAnki: boolean;
  cardFormatCount: number;
  validFormatFlags: boolean[];
  cardFormatIcons: string[];
  excludedDictionaries: string[];
  needsAudio: boolean;
  audioSources: AudioSource[];
  audioEnableAutoplay: boolean;
};

export type AudioSource = {
  name: string;
  url: string;
  isEnabled: boolean;
};

export type MineContent = Record<string, string>;

export type AnkiNoteType = {
  name: string;
  fields: string[];
};

export type AnkiCardFormat = {
  id: string;
  name: string;
  selectedDeck: string | null;
  selectedNoteType: string | null;
  fieldMappings: Record<string, string>;
  tags: string;
  icon: string;
};

export type AnkiSettings = {
  disabled: boolean;
  url: string | null;
  apiKey: string | null;
  duplicateScope: "collection" | "deck" | "deckroot";
  checkAllModels: boolean;
  forceSync: boolean;
  allowDupes: boolean;
  disableShowNotes: boolean;
  compactGlossaries: boolean;
  cardFormats: AnkiCardFormat[];
  availableDecks: string[];
  availableNoteTypes: AnkiNoteType[];
  selectedGlossaryFallback: string;
  showAllHandlebars: boolean;
  audioSources: AudioSource[];
  audioEnableAutoplay: boolean;
  enableLocalAudio: boolean;
  localAudioPath: string | null;
};

export type FontInfo = { name: string; fileName: string };

export type BookSearchResult = {
  chapter: string;
  character: number;
  prefix: string;
  matched: string;
  suffix: string;
  length: number;
};
