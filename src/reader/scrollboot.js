(function () {
  const r = window.hoshiReader;
  const params = new URLSearchParams(location.search);
  const vertical = params.get("mode") === "vertical";
  const mac = navigator.userAgent.includes("Macintosh");
  let fontSize = Number(params.get("fs"));
  let horizontalPadding = Number(params.get("hp"));
  let verticalPadding = Number(params.get("vp"));
  let maxWidth = Number(params.get("mw"));
  let maxHeight = Number(params.get("mh"));
  let justify = params.get("j") === "1";
  let advanced = params.get("adv") === "1";
  let lineHeight = Number(params.get("lh"));
  let charSpacing = Number(params.get("cs"));
  let paraSpacing = Number(params.get("ps"));
  const furiganaMode = params.get("fm");
  const wheelDisabled = params.get("dw") === "1";
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
  const scanDelay = Number(params.get("sdl"));
  const clickLookup = params.get("cl");
  const auxLookupButton = { middle: 1, back: 3, forward: 4 }[clickLookup];
  const isScanKey = (key) => (key.length === 1 ? key.toLowerCase() : key) === scanModifier;
  const scanButton = { "Mouse:Middle": 1, "Mouse:Right": 2, "Mouse:Back": 3, "Mouse:Forward": 4 }[scanModifier];
  const scanButtonMask = [1, 4, 2, 8, 16][scanButton] ?? 0;
  let scanKeyHeld = false;
  function modifierHeld(e) {
    if (scanButtonMask) return (e.buttons & scanButtonMask) !== 0;
    if (scanModifier === "Shift") return e.shiftKey;
    if (scanModifier === "Control") return e.ctrlKey;
    if (scanModifier === "Alt") return e.altKey;
    if (scanModifier === "Meta") return e.metaKey;
    return scanKeyHeld;
  }

  let position = { spine: 0, frac: 0 };
  let restored = false;
  let moving = 0;
  let readerHotkeys = [];
  const mouseHotkeyTokens = { 1: "Mouse:Middle", 2: "Mouse:Right", 3: "Mouse:Back", 4: "Mouse:Forward" };

  if (fontName && fontFile) {
    const fontStyle = document.createElement("style");
    fontStyle.textContent = `@font-face { font-family: "${fontName}"; src: url("/__hoshi/Fonts/${encodeURIComponent(fontFile)}"); }`;
    document.head.appendChild(fontStyle);
  }

  const style = document.createElement("style");
  document.head.appendChild(style);

  function effectivePadding(padding, max, size) {
    if (!max || !size) return padding;
    return Math.max(padding, 100 - (max / size) * 100);
  }

  function applyStyle() {
    const paddingX = effectivePadding(horizontalPadding, maxWidth, window.innerWidth);
    const paddingY = effectivePadding(verticalPadding, maxHeight, window.innerHeight);
    const imgWidth = `calc(${100 - paddingX}vw - 1px)`;
    const imgHeight = `${100 - paddingY}vh`;
    style.textContent = `
      :root { color-scheme: light dark; }
      :root { ${vertical ? `--hoshi-content-width: ${100 - paddingX}vw` : `--hoshi-content-height: ${100 - paddingY}vh`}; }
      html { background: transparent !important; }
      @media (prefers-color-scheme: light) { :root { --hoshi-text-color: #000; } }
      @media (prefers-color-scheme: dark) { :root { --hoshi-text-color: #fff; } }
      html { -webkit-line-box-contain: block glyphs replaced; }
      html, body, hoshi-html, hoshi-body {
        max-height: none !important;
        max-width: none !important;
        margin: 0 !important;
        padding: 0 !important;
        color: var(--hoshi-text-color) !important;
        writing-mode: ${vertical ? "vertical-rl" : "horizontal-tb"} !important;
      }
      html, body {
        overflow: hidden !important;
        height: 100vh !important;
        width: 100vw !important;
      }
      body {
        box-sizing: border-box !important;
        ${
          vertical
            ? `overflow-x: auto !important; width: ${100 - paddingX}vw !important; margin: 0 ${paddingX / 2}vw !important; padding: ${paddingY / 2}vh 0 !important;`
            : `overflow-y: auto !important; height: ${100 - paddingY}vh !important; margin: ${paddingY / 2}vh 0 !important; padding: 0 ${paddingX / 2}vw !important;`
        }
        overflow-anchor: none !important;
        scrollbar-width: none !important;
      }
      body::-webkit-scrollbar { display: none !important; }
      hoshi-section { display: flow-root !important; }
      hoshi-html, hoshi-body {
        display: block !important;
        height: auto !important;
        width: auto !important;
      }
      hoshi-html { background: transparent !important; }
      body, hoshi-body {
        -webkit-text-size-adjust: none !important;
        font-family: ${fontName ? `"${fontName}", ` : ""}"Hiragino Mincho ProN", "Yu Mincho", serif !important;
        ${justify ? "" : "text-align: start !important; hanging-punctuation: allow-end !important; line-break: strict !important;"}
        ${advanced ? `line-height: ${lineHeight} !important; letter-spacing: ${charSpacing / 100}em !important;` : ""}
        ${fontSize ? `font-size: ${fontSize}px !important;` : ""}
      }
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
        object-fit: contain !important;
      }
      svg {
        max-width: ${imgWidth} !important;
        max-height: ${imgHeight} !important;
        width: auto !important;
        height: ${imgHeight} !important;
        display: block !important;
        margin: auto !important;
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
    `;
  }

  applyStyle();
  r.registerCopyText();

  let highlightRange = null;
  let secondaryRange = null;
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
    if (scanButton === 2) return;
    const range = clickLookup === "right" ? secondaryRange : selectionRangeAt(e);
    if (!range) {
      if (clickLookup === "right") return;
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
      textSelected: {
        postMessage: (d) => {
          const section = r.sectionOf(window.hoshiSelection.selection.startNode);
          parent.postMessage(
            { hoshi: "selected", ...d, spine: section.index, normalizedOffset: d.normalizedOffset - section.start },
            "*",
          );
        },
      },
    },
  };

  let lastMouse = null;
  let mouseButtons = 0;
  let scanTimer = 0;
  document.addEventListener(
    "mousemove",
    (e) => {
      mouseButtons = e.buttons;
      lastMouse = { x: e.clientX, y: e.clientY };
      clearTimeout(scanTimer);
      if (e.buttons & ~scanButtonMask) return;
      if (!modifierHeld(e)) {
        if (!scanModifier) {
          scanTimer = setTimeout(() => {
            window.hoshiSelection.selectText(e.clientX, e.clientY, window.scanLength);
          }, scanDelay);
        }
        return;
      }
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
    clearTimeout(scanTimer);
  });
  let mouseDownAt = null;
  let secondaryLookup = false;
  let selectionDismissed = false;
  let pressedSelection = null;
  document.addEventListener("mousedown", (e) => {
    mouseButtons = e.buttons;
    clearTimeout(scanTimer);
    if (e.button === scanButton) {
      e.preventDefault();
      window.hoshiSelection.selectText(e.clientX, e.clientY, window.scanLength);
      return;
    }
    const token = mouseHotkeyTokens[e.button];
    const aux = e.button === auxLookupButton;
    if (token && token !== "Mouse:Right" && !aux && readerHotkeys.includes(token)) {
      e.preventDefault();
      parent.postMessage({ hoshi: "reader-hotkey", key: token }, "*");
      return;
    }
    const secondary = e.button === 2 || (mac && e.ctrlKey);
    secondaryRange = secondary ? selectionRangeAt(e) : null;
    if (secondary ? clickLookup !== "right" || secondaryRange : e.button !== 0 && !aux) {
      if (!window.getSelection().isCollapsed) e.preventDefault();
      return;
    }
    if (aux) e.preventDefault();
    secondaryLookup = secondary;
    mouseDownAt = { x: e.clientX, y: e.clientY };
    selectionDismissed = !window.getSelection().isCollapsed || !!window.hoshiSelection.selection;
    pressedSelection = window.hoshiSelection.selection;
    window.hoshiSelection.clearSelection();
    parent.postMessage({ hoshi: "press" }, "*");
  });
  document.addEventListener("selectstart", (e) => {
    if (secondaryLookup) e.preventDefault();
  });
  document.addEventListener("mouseup", (e) => {
    if (e.button === scanButton) return;
    secondaryLookup = false;
    if (e.button === 2 || (mac && e.ctrlKey)) {
      if (clickLookup === "right" && !secondaryRange) onClick(e, 2);
    } else if (e.button === 0 || e.button === auxLookupButton) {
      onClick(e, e.button);
    }
    setTimeout(() => parent.postMessage({ hoshi: "release" }, "*"));
  });
  document.addEventListener("auxclick", (e) => {
    const token = mouseHotkeyTokens[e.button];
    if (e.button === scanButton || e.button === auxLookupButton) e.preventDefault();
    else if (token && token !== "Mouse:Right" && readerHotkeys.includes(token)) e.preventDefault();
  });
  function onClick(e, button) {
    if (modifierHeld(e)) return;
    if (
      mouseDownAt &&
      (Math.abs(e.clientX - mouseDownAt.x) > 4 || Math.abs(e.clientY - mouseDownAt.y) > 4)
    ) {
      return;
    }
    const anchor = button === 0 && e.target instanceof Element ? e.target.closest("a[href]") : null;
    if (anchor) {
      parent.postMessage({ hoshi: "link", href: anchor.href }, "*");
      return;
    }
    const lookup = button !== 0 || clickLookup === "left";
    if (!lookup && !document.elementFromPoint(e.clientX, e.clientY)?.closest("ruby.furigana-hidden")) {
      parent.postMessage({ hoshi: "lookup-miss", dismissed: selectionDismissed }, "*");
      return;
    }
    const hit = pressedSelection && window.hoshiSelection.getCharacterAtPoint(e.clientX, e.clientY);
    if (hit && hit.node === pressedSelection.startNode && hit.offset === pressedSelection.startOffset) {
      parent.postMessage({ hoshi: "lookup-miss", dismissed: true }, "*");
      return;
    }
    const selected = window.hoshiSelection.selectText(e.clientX, e.clientY, window.scanLength);
    if (!selected && button === 0) {
      parent.postMessage({ hoshi: "lookup-miss", dismissed: selectionDismissed }, "*");
    }
  }
  document.addEventListener("click", (e) => {
    if (e.target instanceof Element && e.target.closest("a[href]")) e.preventDefault();
  });

  function report(hoshi, jump) {
    position = r.calculateProgress();
    parent.postMessage({ hoshi, ...position, jump }, "*");
  }

  async function settle(move) {
    moving++;
    await move?.();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    moving--;
  }

  function jumped() {
    settle();
    report("progress", true);
  }

  function positioned() {
    document.documentElement.style.transition = "opacity 200ms ease";
    document.documentElement.style.opacity = "1";
    parent.postMessage({ hoshi: "positioned" }, "*");
  }

  function reflow() {
    if (restored) settle(() => r.restoreProgress(position.spine, position.frac));
  }

  let scrollTimer = 0;
  let scrollReported = 0;
  document.body.addEventListener(
    "scroll",
    () => {
      if (!restored || moving) return;
      const now = performance.now();
      if (now - scrollReported >= 50) {
        scrollReported = now;
        report("scroll");
      }
      clearTimeout(scrollTimer);
      scrollTimer = setTimeout(() => report("progress"), 150);
    },
    { passive: true },
  );

  window.addEventListener("resize", () => {
    if (maxWidth || maxHeight) applyStyle();
    reflow();
  });

  function applyCues(cues) {
    if (!cues) return;
    r.applySasayakiCues(
      cues.flatMap((chapter, spine) =>
        chapter.map((cue) => ({ ...cue, start: cue.start + r.sections[spine].start })),
      ),
    );
  }

  function applyHighlights(highlights) {
    if (!highlights) return;
    window.hoshiHighlights.applyHighlights(
      highlights.flatMap((chapter, spine) =>
        chapter.map((highlight) => ({ ...highlight, offset: highlight.offset + r.sections[spine].rawStart })),
      ),
    );
  }

  function turn(dir) {
    window.hoshiHighlights.clearSearchHighlight();
    const step = (vertical ? document.body.clientWidth : document.body.clientHeight) * 0.8 * (dir === "forward" ? 1 : -1);
    document.body.scrollBy({ left: vertical ? -step : 0, top: vertical ? 0 : step, behavior: "smooth" });
  }

  function wheel(dx, dy) {
    if (!wheelDisabled) r.scrollBy(vertical && Math.abs(dx) > Math.abs(dy) ? -dx : dy);
  }

  const scrollKeys = { ArrowDown: "forward", PageDown: "forward", ArrowUp: "backward", PageUp: "backward" };
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
    if (e.ctrlKey || e.altKey || e.metaKey) return;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const control = e.target.closest("button, a, [role='button'], summary");
    if (readerHotkeys.includes(key) && !(control && [" ", "Enter", "Tab"].includes(e.key))) {
      e.preventDefault();
      parent.postMessage({ hoshi: "reader-hotkey", key, repeat: e.repeat }, "*");
      return;
    }
    const dir = e.key === " " ? (e.shiftKey ? "backward" : "forward") : scrollKeys[e.key];
    if (dir) {
      e.preventDefault();
      turn(dir);
    }
  });

  window.addEventListener(
    "wheel",
    (e) => {
      if (!vertical && !wheelDisabled) return;
      e.preventDefault();
      wheel(e.deltaX, e.deltaY);
    },
    { passive: false },
  );

  window.addEventListener("message", (e) => {
    const m = e.data;
    switch (m?.hoshi) {
      case "reader-hotkeys":
        readerHotkeys = m.keys;
        break;
      case "turn":
        turn(m.dir);
        break;
      case "wheel":
        wheel(m.dx, m.dy);
        break;
      case "restore":
        applyCues(m.cues);
        applyHighlights(m.highlights);
        restored = true;
        position = { spine: m.spine, frac: m.progress };
        settle(() => r.restoreProgress(m.spine, m.progress)).then(positioned);
        break;
      case "fragment":
        applyCues(m.cues);
        applyHighlights(m.highlights);
        restored = true;
        settle(() => r.jumpToFragment(m.spine, m.fragment)).then(() => {
          report("progress", true);
          positioned();
        });
        break;
      case "restyle":
        fontSize = m.fs;
        horizontalPadding = m.hp;
        verticalPadding = m.vp;
        maxWidth = m.mw;
        maxHeight = m.mh;
        justify = m.j;
        advanced = m.adv;
        lineHeight = m.lh;
        charSpacing = m.cs;
        paraSpacing = m.ps;
        applyStyle();
        reflow();
        break;
      case "create-highlight": {
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(highlightRange);
        const section = r.sectionOf(highlightRange.startContainer);
        highlightRange = null;
        const result = window.hoshiHighlights.createHighlight(m.color, m.id);
        if (!result) break;
        if (!result.id) {
          result.spine = section.index;
          result.start -= section.start;
          result.offset -= section.rawStart;
        }
        parent.postMessage({ hoshi: "highlight-created", id: m.id, color: m.color, result }, "*");
        break;
      }
      case "remove-highlight":
        window.hoshiHighlights.removeHighlight(m.id);
        break;
      case "search-highlight":
        window.hoshiHighlights.showSearchHighlight(m.offset + r.sections[m.spine].start, m.length);
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
      case "sasayaki-highlight":
        if (r.highlightSasayakiCue(m.id, m.reveal)) jumped();
        break;
      case "sasayaki-clear":
        r.clearSasayakiCue();
        break;
      case "sasayaki-cues":
        applyCues(m.cues);
        break;
      case "sasayaki-image": {
        const scrolled = r.scrollToSasayakiImage(m.spine, m.index);
        if (scrolled) jumped();
        parent.postMessage({ hoshi: "sasayaki-image-result", paused: scrolled !== null }, "*");
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

  r.loadSections(window.hoshiSpine)
    .then(() => {
      if (furiganaMode === "Toggle") {
        document.querySelectorAll("ruby").forEach((ruby) => {
          if (ruby.querySelector("rt")) ruby.classList.add("furigana-hidden");
        });
      } else if (furiganaMode === "Hidden") {
        document.querySelectorAll("rt").forEach((rt) => rt.remove());
      }
      return setupImages();
    })
    .then(() => {
      r.measureSections();
      parent.postMessage({ hoshi: "ready" }, "*");
    });
})();
