import type { SelectionRect } from "./types";

export type PopupPlacement = {
  left: number;
  top: number;
  width: number;
  height: number;
};

export function calculatePopupLayout(
  selectionRect: SelectionRect,
  screenSize: { width: number; height: number },
  maxWidth: number,
  maxHeight: number,
  isVertical: boolean,
  topInset = 0,
  bottomInset = 0,
): PopupPlacement {
  const popupPadding = 4;
  const screenBorderPadding = 6;

  const minX = selectionRect.x;
  const minY = selectionRect.y;
  const maxX = selectionRect.x + selectionRect.width;
  const maxY = selectionRect.y + selectionRect.height;

  const spaceLeft = minX - popupPadding;
  const spaceRight = screenSize.width - maxX - popupPadding;
  const showOnRight = spaceRight >= spaceLeft || spaceRight >= maxWidth;
  const spaceAbove = minY - topInset - popupPadding;
  const spaceBelow = screenSize.height - bottomInset - maxY - popupPadding;

  const width = isVertical
    ? Math.min(Math.max(spaceLeft, spaceRight) - screenBorderPadding, maxWidth)
    : Math.min(screenSize.width - screenBorderPadding * 2, maxWidth);

  const height = isVertical
    ? maxHeight
    : Math.min(Math.max(spaceAbove, spaceBelow) - screenBorderPadding, maxHeight);

  const showBelow = spaceBelow >= height;

  let x: number;
  let y: number;
  if (isVertical) {
    if (showOnRight) {
      x = maxX + popupPadding + width / 2;
    } else {
      x = minX - popupPadding - width / 2;
    }
    x = Math.max(width / 2, Math.min(x, screenSize.width - width / 2));

    y = minY + height / 2;
    y = Math.max(
      height / 2 + screenBorderPadding + topInset,
      Math.min(y, screenSize.height - bottomInset - height / 2 - screenBorderPadding),
    );
  } else {
    x = minX + width / 2;
    x = Math.max(
      width / 2 + screenBorderPadding,
      Math.min(x, screenSize.width - width / 2 - screenBorderPadding),
    );

    if (showBelow) {
      y = maxY + popupPadding + height / 2;
    } else {
      y = minY - popupPadding - height / 2;
    }
    y = Math.max(
      height / 2 + topInset + screenBorderPadding,
      Math.min(y, screenSize.height - bottomInset - height / 2 - screenBorderPadding),
    );
  }

  return { left: x - width / 2, top: y - height / 2, width, height };
}
