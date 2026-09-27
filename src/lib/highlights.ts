export const highlightColors = {
  yellow: [239, 209, 56],
  green: [152, 220, 129],
  blue: [149, 185, 255],
  pink: [255, 155, 180],
  purple: [197, 175, 251],
} as const;

export type HighlightColor = keyof typeof highlightColors;

export type BookHighlight = {
  id: string;
  character: number;
  offset: number;
  text: string;
  textFurigana?: string | null;
  color: HighlightColor;
  createdAt: number;
};
