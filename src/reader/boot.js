(function () {
  const r = window.hoshiReader;
  const params = new URLSearchParams(location.search);
  const vertical = params.get("mode") === "vertical";
  const chromium = navigator.userAgent.includes("Chrome/");
  const mac = navigator.userAgent.includes("Macintosh");
  let fontSize = Number(params.get("fs"));
  let horizontalPadding = Number(params.get("hp"));
  let verticalPadding = Number(params.get("vp"));
  let maxWidth = Number(params.get("mw"));
  let maxHeight = Number(params.get("mh"));
  let justify = params.get("j") === "1";
  let avoidBreak = params.get("apb") === "1";
  let advanced = params.get("adv") === "1";
  let lineHeight = Number(params.get("lh"));
  let charSpacing = Number(params.get("cs"));
  let paraSpacing = Number(params.get("ps"));
  const furiganaMode = params.get("fm");
  const paragraphMode = params.get("pm") === "1";
  let textSpeed = params.get("ta") === "1" ? Number(params.get("ts")) : 0;
  let clickAdvance = paragraphMode && params.get("ca") === "1";
  const blurImages = params.get("bi") === "1";
  const fontName = params.get("font");
  const fontFile = params.get("ffile");
  const sasayakiTextColor = params.get("stc");
  const sasayakiBackgroundColor = params.get("sbc");
  if (sasayakiTextColor) {
    document.documentElement.style.setProperty("--hoshi-sasayaki-text-color", sasayakiTextColor);
  }
  if (sasayakiBackgroundColor) {
    document.documentElement.style.setProperty(
      "--hoshi-sasayaki-background-color",
      sasayakiBackgroundColor,
    );
  }
  const textColor = params.get("tc");
  if (textColor) {
    document.documentElement.style.setProperty("--hoshi-text-color", "#" + textColor);
  }

  window.scanLength = Number(params.get("sl"));
  window.scanNonJapaneseText = params.get("snj") !== "0";
  const scanModifier = params.get("mod");
  const clickLookup = params.get("cl");
  const isScanKey = (key) => (key.length === 1 ? key.toLowerCase() : key) === scanModifier;
  let scanKeyHeld = false;
  function modifierHeld(e) {
    if (scanModifier === "Shift") return e.shiftKey;
    if (scanModifier === "Control") return e.ctrlKey;
    if (scanModifier === "Alt") return e.altKey;
    if (scanModifier === "Meta") return e.metaKey;
    return scanKeyHeld;
  }

  let position = 0;
  let restored = false;
  let readerHotkeys = [];
  const mouseHotkeyTokens = { 1: "Mouse:Middle", 2: "Mouse:Right", 3: "Mouse:Back", 4: "Mouse:Forward" };

  if (fontName && fontFile) {
    const fontStyle = document.createElement("style");
    fontStyle.textContent = `@font-face { font-family: "${fontName}"; src: url("/__hoshi/Fonts/${encodeURIComponent(fontFile)}"); }`;
    document.head.appendChild(fontStyle);
  }

  const style = document.createElement("style");
  document.head.appendChild(style);
  let spacer = null;

  function effectivePadding(padding, max, size) {
    if (!max || !size) return padding;
    return Math.max(padding, 100 - (max / size) * 100);
  }

  function applyStyle() {
    const paddingX = effectivePadding(horizontalPadding, maxWidth, window.innerWidth);
    const paddingY = effectivePadding(verticalPadding, maxHeight, window.innerHeight);
    const overlap = vertical && !chromium ? fontSize : 0;
    const imgWidth = `calc(${100 - paddingX}vw - 1px)`;
    const imgHeight = vertical
      ? `calc(${100 - paddingY}vh - ${(overlap * (100 - paddingY)) / 100}px)`
      : `${100 - paddingY}vh`;
    const columns = vertical && chromium
      ? `column-width: 100vh !important; column-height: ${100 - paddingX}vw !important; column-wrap: wrap !important; row-gap: ${paddingX}vw !important; column-gap: 0 !important;`
      : `-webkit-column-axis: horizontal !important; column-width: 100vw !important; column-gap: ${paddingX}vw !important;`;
    style.textContent = `
      :root { color-scheme: light dark; }
      html { background: transparent !important; }
      @media (prefers-color-scheme: light) { :root { --hoshi-text-color: #000; } }
      @media (prefers-color-scheme: dark) { :root { --hoshi-text-color: #fff; } }
      html { -webkit-line-box-contain: block glyphs replaced; }
      html, body {
        overflow: hidden !important;
        height: 100vh !important;
        width: 100vw !important;
        margin: 0 !important;
        padding: 0 !important;
        color: var(--hoshi-text-color) !important;
        writing-mode: ${vertical ? "vertical-rl" : "horizontal-tb"} !important;
      }
      body * {
        column-count: auto !important;
        -webkit-column-count: auto !important;
      }
      body {
        box-sizing: border-box !important;
        ${columns}
        column-fill: auto !important;
        -webkit-text-size-adjust: none !important;
        font-family: ${fontName ? `"${fontName}", ` : ""}"Hiragino Mincho ProN", "Yu Mincho", serif !important;
        ${justify ? "" : "text-align: start !important; hanging-punctuation: allow-end !important; line-break: strict !important;"}
        ${advanced ? `line-height: ${lineHeight} !important; letter-spacing: ${charSpacing / 100}em !important;` : ""}
        padding: ${paddingY / 2}vh ${paddingX / 2}vw !important;
        ${overlap ? `padding-bottom: calc(${paddingY / 2}vh + ${overlap}px) !important;` : ""}
        ${fontSize ? `font-size: ${fontSize}px !important;` : ""}
      }
      ${avoidBreak ? "p { break-inside: avoid !important; -webkit-column-break-inside: avoid !important; }" : ""}
      ${
        advanced
          ? vertical
            ? `p { margin-right: ${paraSpacing}em !important; margin-left: ${paraSpacing}em !important; }`
            : `p { margin-top: ${paraSpacing}em !important; margin-bottom: ${paraSpacing}em !important; }`
          : ""
      }
      .blur-wrapper {
        display: table;
        margin: auto;
        line-height: 0;
        overflow: hidden;
      }
      img.block-img.blurred,
      svg.blurred {
        filter: blur(24px) !important;
        clip-path: inset(0);
        cursor: pointer;
      }
      img.block-img:not(.blurred),
      svg:has(image):not(.blurred) {
        cursor: zoom-in;
      }
      ${furiganaMode === "Dimmed" ? "ruby > rt, ruby > rp { opacity: 0.4 !important; }" : ""}
      ruby.furigana-hidden > rp,
      ruby.furigana-hidden > rt > * {
        visibility: hidden !important;
      }
      ruby.furigana-hidden {
        cursor: pointer;
      }
      ruby.furigana-hidden > rt {
        color: transparent !important;
        background-image: linear-gradient(rgba(150, 150, 150, 0.75), rgba(150, 150, 150, 0.75)) !important;
        background-size: ${vertical ? "0.14em 100%" : "100% 0.14em"} !important;
        background-position: ${vertical ? "left center" : "center bottom"} !important;
        background-repeat: no-repeat !important;
      }
      img.block-img {
        max-width: ${imgWidth} !important;
        max-height: ${imgHeight} !important;
        width: auto !important;
        height: auto !important;
        display: block !important;
        margin: auto !important;
        break-inside: avoid !important;
        -webkit-column-break-inside: avoid !important;
        object-fit: contain !important;
      }
      svg {
        max-width: ${imgWidth} !important;
        max-height: ${imgHeight} !important;
        width: 100% !important;
        height: 100% !important;
        display: block !important;
        margin: auto !important;
        break-inside: avoid !important;
        -webkit-column-break-inside: avoid !important;
      }
      .hoshi-highlight-yellow { background-color: rgba(239, 209, 56, 0.35) !important; }
      .hoshi-highlight-green { background-color: rgba(152, 220, 129, 0.35) !important; }
      .hoshi-highlight-blue { background-color: rgba(149, 185, 255, 0.35) !important; }
      .hoshi-highlight-pink { background-color: rgba(255, 155, 180, 0.35) !important; }
      .hoshi-highlight-purple { background-color: rgba(197, 175, 251, 0.35) !important; }
      ::highlight(hoshi-search) {
        background-color: rgba(100, 160, 255, 0.4) !important;
        color: inherit;
      }
      ::selection {
        background-color: rgba(160, 160, 160, 0.4) !important;
        color: inherit;
      }
      ruby > rt, ruby > rp {
        -webkit-user-select: none !important;
        user-select: none !important;
      }
      .hoshi-sasayaki-cue.hoshi-sasayaki-active {
        color: var(--hoshi-sasayaki-text-color) !important;
        background-color: var(--hoshi-sasayaki-background-color) !important;
      }
      ${
        paragraphMode
          ? `body { font-kerning: none !important; }
      p.hoshi-paragraph {
        margin-block-start: 0 !important;
        break-before: column !important;
        -webkit-column-break-before: always !important;
      }
      ::highlight(hoshi-animation) { color: transparent !important; }`
          : ""
      }
    `;

    if (spacer) {
      spacer.remove();
      spacer = null;
    }
    r.spacer = null;
    if (chromium) {
      return;
    }
    spacer = document.createElement("div");
    spacer.style.cssText = vertical
      ? "display:block;break-inside:avoid;width:100%"
      : `display:block;break-inside:avoid;height:100%;width:${paddingX / 2}vw`;
    document.body.appendChild(spacer);
    r.spacer = spacer;
  }

  function syncPageSize() {
    const rect = document.documentElement.getBoundingClientRect();
    r.pageWidth = rect.width || window.innerWidth;
    r.pageHeight = rect.height || window.innerHeight;
  }

  function sized() {
    syncPageSize();
    return r.pageWidth > 0 && r.pageHeight > 0;
  }

  function whenSized() {
    if (sized()) return Promise.resolve();
    return new Promise((resolve) => {
      const observer = new ResizeObserver(() => {
        if (!sized()) return;
        observer.disconnect();
        clearTimeout(timer);
        resolve();
      });
      observer.observe(document.documentElement);
      const timer = setTimeout(() => {
        observer.disconnect();
        resolve();
      }, 1500);
    });
  }

  function nodeAtProgress(p) {
    let total = 0;
    let node;
    let walker = r.createWalker();
    while ((node = walker.nextNode())) total += r.countChars(node.textContent);
    if (total <= 0) return null;
    const target = Math.ceil(total * p);
    let sum = 0;
    walker = r.createWalker();
    while ((node = walker.nextNode())) {
      sum += r.countChars(node.textContent);
      if (sum > target) return node;
    }
    return null;
  }

  function alignToNode(node) {
    const ctx = r.getScrollContext();
    if (ctx.pageSize <= 0 || !node) return;
    const range = document.createRange();
    range.setStart(node, 0);
    range.setEnd(node, Math.min(1, node.textContent.length));
    const rect = r.getRect(range);
    const cur = Math.abs(ctx.scrollEl.scrollLeft);
    const anchor =
      (ctx.vertical ? r.pageWidth - (rect.left + rect.right) / 2 : (rect.left + rect.right) / 2) + cur;
    const target = r.alignToPage(ctx, anchor);
    window.lastPageScroll = target;
    r.setScrollOffset(ctx, target);
    showAnchorMarker(node);
  }

  function showAnchorMarker(node) {
    if (!node) return;
    let container = node;
    if (container.parentElement) {
      container = container.parentElement.closest("p") || container.parentElement;
    }
    const paraRange = document.createRange();
    paraRange.selectNode(container);
    const paraRect = paraRange.getBoundingClientRect();
    const range = document.createRange();
    range.setStart(node, 0);
    range.setEnd(node, Math.min(1, node.textContent.length));
    const rect = r.getRect(range);
    if (!rect || (rect.width === 0 && rect.height === 0)) return;
    const bodyStyle = window.getComputedStyle(document.body);
    parent.postMessage(
      {
        hoshi: "marker",
        x: vertical ? paraRect.right : paraRect.left,
        y: (rect.top + rect.bottom) / 2,
        inset: parseFloat(vertical ? bodyStyle.paddingTop : bodyStyle.paddingLeft) || 0,
      },
      "*",
    );
  }

  function updateMarker() {
    showAnchorMarker(nodeAtProgress(position));
  }

  applyStyle();
  syncPageSize();
  r.registerCopyText();

  if (furiganaMode === "Toggle") {
    document.querySelectorAll("ruby").forEach((ruby) => {
      if (ruby.querySelector("rt")) ruby.classList.add("furigana-hidden");
    });
  } else if (furiganaMode === "Hidden") {
    document.querySelectorAll("rt").forEach((rt) => rt.remove());
  }

  let highlightRange = null;
  function selectionRangeAt(e) {
    const selection = window.getSelection();
    const range = selection && !selection.isCollapsed && !window.hoshiSelection.selection
      ? selection.getRangeAt(0)
      : null;
    const onSelection = range && [...range.getClientRects()].some((rect) =>
      e.clientX >= rect.left - 4 && e.clientX <= rect.right + 4 &&
      e.clientY >= rect.top - 4 && e.clientY <= rect.bottom + 4,
    );
    return onSelection ? range : null;
  }
  document.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const range = selectionRangeAt(e);
    if (!range) {
      if (clickLookup === "right") {
        onClick(e, 2);
        return;
      }
      window.hoshiSelection.clearSelection();
      if (readerHotkeys.includes("Mouse:Right")) {
        parent.postMessage({ hoshi: "reader-hotkey", key: "Mouse:Right" }, "*");
      } else {
        parent.postMessage({ hoshi: "press" }, "*");
      }
      return;
    }
    highlightRange = range.cloneRange();
    parent.postMessage({ hoshi: "selection-menu" }, "*");
  });

  window.webkit = {
    messageHandlers: {
      restoreCompleted: {
        postMessage: () => {
          document.documentElement.style.transition = "opacity 200ms ease";
          document.documentElement.style.opacity = "1";
          animateText();
          parent.postMessage({ hoshi: "positioned" }, "*");
        },
      },
      textSelected: { postMessage: (d) => parent.postMessage({ hoshi: "selected", ...d }, "*") },
    },
  };

  let lastMouse = null;
  let mouseButtons = 0;
  document.addEventListener(
    "mousemove",
    (e) => {
      mouseButtons = e.buttons;
      lastMouse = { x: e.clientX, y: e.clientY };
      if (e.buttons || !modifierHeld(e)) return;
      if (window.hoshiParagraph.finishTextAnimation()) return;
      window.hoshiSelection.selectText(e.clientX, e.clientY, window.scanLength);
    },
    true,
  );
  document.addEventListener(
    "keydown",
    (e) => {
      if (!isScanKey(e.key)) return;
      scanKeyHeld = true;
      if (e.repeat || !lastMouse || mouseButtons) return;
      if (window.hoshiParagraph.finishTextAnimation()) return;
      window.hoshiSelection.selectText(lastMouse.x, lastMouse.y, window.scanLength);
    },
    true,
  );
  document.addEventListener(
    "mouseup",
    (e) => {
      mouseButtons = e.buttons;
    },
    true,
  );
  document.addEventListener(
    "keyup",
    (e) => {
      if (isScanKey(e.key)) scanKeyHeld = false;
    },
    true,
  );
  window.addEventListener("blur", () => {
    scanKeyHeld = false;
    mouseButtons = 0;
  });
  document.documentElement.addEventListener("mouseleave", () => {
    lastMouse = null;
  });
  let mouseDownAt = null;
  document.addEventListener("mousedown", (e) => {
    mouseButtons = e.buttons;
    const token = mouseHotkeyTokens[e.button];
    if (token && token !== "Mouse:Right" && readerHotkeys.includes(token)) {
      e.preventDefault();
      parent.postMessage({ hoshi: "reader-hotkey", key: token }, "*");
      return;
    }
    const secondary = e.button === 2 || (mac && e.ctrlKey);
    if (secondary ? clickLookup !== "right" || selectionRangeAt(e) : e.button !== 0) {
      if (!window.getSelection().isCollapsed) e.preventDefault();
      return;
    }
    mouseDownAt = { x: e.clientX, y: e.clientY };
    if (clickAdvance && e.detail > 1) e.preventDefault();
    window.hoshiSelection.clearSelection();
    parent.postMessage({ hoshi: "press" }, "*");
  });
  document.addEventListener("mouseup", () => {
    setTimeout(() => parent.postMessage({ hoshi: "release" }, "*"));
  });
  document.addEventListener("auxclick", (e) => {
    const token = mouseHotkeyTokens[e.button];
    if (token && token !== "Mouse:Right" && readerHotkeys.includes(token)) e.preventDefault();
  });
  function onClick(e, button) {
    if (modifierHeld(e)) return;
    const anchor = button === 0 && e.target instanceof Element ? e.target.closest("a[href]") : null;
    if (button !== (clickLookup === "right" ? 2 : 0) && !anchor) return;
    if (window.hoshiParagraph.finishTextAnimation()) return;
    if (anchor) {
      e.preventDefault();
      parent.postMessage({ hoshi: "link", href: anchor.href }, "*");
      return;
    }
    if (
      mouseDownAt &&
      (Math.abs(e.clientX - mouseDownAt.x) > 4 || Math.abs(e.clientY - mouseDownAt.y) > 4)
    ) {
      return;
    }
    if (clickLookup === "off" && !document.elementFromPoint(e.clientX, e.clientY)?.closest("ruby.furigana-hidden")) {
      parent.postMessage({ hoshi: "lookup-miss" }, "*");
      return;
    }
    const selected = window.hoshiSelection.selectText(e.clientX, e.clientY, window.scanLength);
    if (!selected) {
      parent.postMessage({ hoshi: "lookup-miss" }, "*");
    }
  }
  document.addEventListener("click", (e) => {
    if (mac && e.ctrlKey) return;
    onClick(e, 0);
  });

  let resizeAnchor = null;
  let resizeTimer = 0;
  let lastPageWidth = 0;
  let lastPageHeight = 0;

  function handleViewportChange() {
    if (!sized()) return;
    if (r.pageWidth === lastPageWidth && r.pageHeight === lastPageHeight) return;
    lastPageWidth = r.pageWidth;
    lastPageHeight = r.pageHeight;
    if (maxWidth || maxHeight) applyStyle();
    layoutParagraphs();
    if (!restored) return;
    if (!resizeAnchor) resizeAnchor = nodeAtProgress(position);
    alignToNode(resizeAnchor);
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => (resizeAnchor = null), 150);
  }

  window.addEventListener("resize", handleViewportChange);
  new ResizeObserver(handleViewportChange).observe(document.documentElement);

  function applyCues(cues) {
    if (!cues) return;
    r.applySasayakiCues(cues);
  }

  function restyle(m) {
    const anchor = nodeAtProgress(position);
    fontSize = m.fs;
    horizontalPadding = m.hp;
    verticalPadding = m.vp;
    maxWidth = m.mw;
    maxHeight = m.mh;
    justify = m.j;
    avoidBreak = m.apb;
    advanced = m.adv;
    lineHeight = m.lh;
    charSpacing = m.cs;
    paraSpacing = m.ps;
    applyStyle();
    syncPageSize();
    layoutParagraphs();
    alignToNode(anchor);
  }

  function layoutParagraphs() {
    if (!paragraphMode) return;
    window.hoshiParagraph.finishTextAnimation();
    window.hoshiParagraph.layoutParagraphs();
  }

  function animateText() {
    if (paragraphMode && textSpeed) window.hoshiParagraph.animateText(textSpeed);
  }

  function turn(dir) {
    if (!sized()) return;
    window.hoshiHighlights.clearSearchHighlight();
    window.hoshiParagraph.finishTextAnimation();
    if (r.paginate(dir) === "limit") {
      parent.postMessage({ hoshi: "boundary", dir }, "*");
    } else {
      if (dir === "forward") animateText();
      position = r.calculateProgress();
      updateMarker();
      parent.postMessage({ hoshi: "progress", frac: position, dir }, "*");
    }
  }

  function keyDirection(key) {
    if (key === "ArrowRight") return vertical ? "backward" : "forward";
    if (key === "ArrowLeft") return vertical ? "forward" : "backward";
    return null;
  }

  window.addEventListener("keydown", (e) => {
    if (e.defaultPrevented || e.isComposing || e.target.closest("input, textarea, select, [contenteditable]")) return;
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      parent.postMessage({ hoshi: "search" }, "*");
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      parent.postMessage({ hoshi: "escape" }, "*");
      return;
    }
    if (!e.ctrlKey && !e.altKey && !e.metaKey) {
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      const control = e.target.closest("button, a, [role='button'], summary");
      if (readerHotkeys.includes(key) && !(control && [" ", "Enter", "Tab"].includes(e.key))) {
        e.preventDefault();
        if (!e.repeat) parent.postMessage({ hoshi: "reader-hotkey", key }, "*");
        return;
      }
    }
    const dir = keyDirection(e.key);
    if (!dir) return;
    e.preventDefault();
    turn(dir);
  });

  window.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      parent.postMessage({ hoshi: "wheel", dx: e.deltaX, dy: e.deltaY }, "*");
    },
    { passive: false },
  );

  function commitProgress(value) {
    position = value;
    updateMarker();
    parent.postMessage({ hoshi: "progress", frac: position, jump: true }, "*");
  }

  window.addEventListener("message", (e) => {
    const m = e.data;
    switch (m?.hoshi) {
      case "reader-hotkeys":
        readerHotkeys = m.keys;
        break;
      case "turn":
        turn(m.dir);
        break;
      case "restore":
        position = m.progress;
        applyCues(m.cues);
        if (m.highlights) window.hoshiHighlights.applyHighlights(m.highlights);
        r.restoreProgress(m.progress).then(() => {
          restored = true;
          requestAnimationFrame(() => requestAnimationFrame(updateMarker));
        });
        break;
      case "fragment":
        applyCues(m.cues);
        if (m.highlights) window.hoshiHighlights.applyHighlights(m.highlights);
        r.jumpToFragment(m.fragment).then(() => {
          restored = true;
          commitProgress(r.calculateProgress());
        });
        break;
      case "restyle":
        restyle(m);
        break;
      case "create-highlight": {
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(highlightRange);
        highlightRange = null;
        const result = window.hoshiHighlights.createHighlight(m.color, m.id);
        if (result) parent.postMessage({ hoshi: "highlight-created", id: m.id, color: m.color, result }, "*");
        break;
      }
      case "remove-highlight":
        window.hoshiHighlights.removeHighlight(m.id);
        break;
      case "search-highlight":
        window.hoshiHighlights.showSearchHighlight(m.offset, m.length);
        break;
      case "highlight":
        window.hoshiSelection.highlightSelection(m.count);
        break;
      case "textcolor":
        if (m.tc) {
          document.documentElement.style.setProperty("--hoshi-text-color", m.tc);
        } else {
          document.documentElement.style.removeProperty("--hoshi-text-color");
        }
        break;
      case "clear-selection":
        window.hoshiSelection.clearSelection();
        break;
      case "sasayaki-colors":
        document.documentElement.style.setProperty("--hoshi-sasayaki-text-color", m.text);
        document.documentElement.style.setProperty(
          "--hoshi-sasayaki-background-color",
          m.background,
        );
        break;
      case "sasayaki-highlight": {
        const progress = r.highlightSasayakiCue(m.id, m.reveal);
        if (typeof progress === "number") commitProgress(progress);
        break;
      }
      case "sasayaki-clear":
        r.clearSasayakiCue();
        break;
      case "sasayaki-cues":
        applyCues(m.cues);
        break;
      case "text-animation":
        textSpeed = m.speed;
        if (!textSpeed) window.hoshiParagraph.finishTextAnimation();
        break;
      case "click-advance":
        clickAdvance = paragraphMode && m.enabled;
        break;
      case "page-cues":
        parent.postMessage({ hoshi: "page-cues", ids: window.hoshiParagraph.pageSasayakiCues(), play: m.play }, "*");
        break;
      case "sasayaki-image": {
        const result = r.scrollToSasayakiImage(m.index);
        if (typeof result?.progress === "number") commitProgress(result.progress);
        parent.postMessage({ hoshi: "sasayaki-image-result", paused: result !== null }, "*");
        break;
      }
    }
  });

  function setupImage(el, src, wrap, blurred = el) {
    let target = el;
    if (blurImages) {
      blurred.classList.add("blurred");
      if (wrap) {
        target = document.createElement("div");
        target.className = "blur-wrapper";
        blurred.before(target);
        target.append(blurred);
      }
    }
    target.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      blurred.classList.remove("blurred");
    });
    target.addEventListener("dblclick", (e) => {
      e.preventDefault();
      e.stopPropagation();
      parent.postMessage({ hoshi: "open-image", url: new URL(src, document.baseURI).href }, "*");
    });
  }

  function setupImages() {
    document
      .querySelectorAll('svg[preserveAspectRatio="none"]')
      .forEach((svg) => svg.removeAttribute("preserveAspectRatio"));
    document.querySelectorAll("svg").forEach((svg) => {
      const image = svg.querySelector("image");
      if (image) setupImage(image, image.href.baseVal, false, svg);
    });
    const images = document.querySelectorAll("img");
    const promises = Array.from(images).map(
      (img) =>
        new Promise((resolve) => {
          function processImg() {
            const isGaiji =
              img.classList.contains("gaiji") || img.classList.contains("gaiji-line");
            if (!isGaiji && (img.naturalWidth > 256 || img.naturalHeight > 256)) {
              img.classList.add("block-img");
              setupImage(img, img.src, true);
            }
            resolve();
          }
          if (img.complete) {
            processImg();
          } else {
            img.onload = processImg;
            img.onerror = () => resolve();
          }
        }),
    );
    return Promise.all(promises).then(() => new Promise((r) => setTimeout(r, 50)));
  }

  setupImages()
    .then(whenSized)
    .then(() => paragraphMode && r.awaitFonts().then(layoutParagraphs))
    .then(() => parent.postMessage({ hoshi: "ready" }, "*"));
})();
