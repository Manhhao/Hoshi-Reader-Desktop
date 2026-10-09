(function () {
    const reader = window.hoshiReader;
    const input = window.hoshiInput;
    const params = new URLSearchParams(location.search);
    const root = document.documentElement;

    const vertical = params.get('mode') === 'vertical';
    const furiganaMode = params.get('fm');
    const wheelDisabled = params.get('dw') === '1';
    const fontName = params.get('font');
    const fontFile = params.get('ffile');
    const textColor = params.get('tc');
    const sasayakiTextColor = params.get('stc');
    const sasayakiBackgroundColor = params.get('sbc');

    const layout = {
        fontSize: Number(params.get('fs')),
        horizontalPadding: Number(params.get('hp')),
        verticalPadding: Number(params.get('vp')),
        maxWidth: Number(params.get('mw')),
        maxHeight: Number(params.get('mh')),
        justify: params.get('j') === '1',
        advanced: params.get('adv') === '1',
        lineHeight: Number(params.get('lh')),
        charSpacing: Number(params.get('cs')),
        paraSpacing: Number(params.get('ps'))
    };

    const style = document.createElement('style');

    let position = { spine: 0, frac: 0 };
    let restored = false;
    let moving = 0;
    let scrollTimer = 0;
    let scrollReported = 0;

    function post(message) {
        parent.postMessage(message, '*');
    }

    function effectivePadding(padding, max, size) {
        if (!max || !size) {
            return padding;
        }
        return Math.max(padding, 100 - (max / size) * 100);
    }

    function applyStyle() {
        const paddingX = effectivePadding(layout.horizontalPadding, layout.maxWidth, window.innerWidth);
        const paddingY = effectivePadding(layout.verticalPadding, layout.maxHeight, window.innerHeight);
        const writingMode = vertical ? 'vertical-rl' : 'horizontal-tb';
        const imgWidth = `calc(${100 - paddingX}vw - 1px)`;
        const imgHeight = `${100 - paddingY}vh`;
        const fontFamily = `${fontName ? `"${fontName}", ` : ''}"Hiragino Mincho ProN", "Yu Mincho", serif`;
        const furiganaLine = 'rgba(150, 150, 150, 0.75)';

        const contentSizeCss = vertical
            ? `--hoshi-content-width: ${100 - paddingX}vw;`
            : `--hoshi-content-height: ${100 - paddingY}vh;`;

        const scrollCss = vertical
            ? `
                overflow-x: auto !important;
                width: ${100 - paddingX}vw !important;
                margin: 0 ${paddingX / 2}vw !important;
                padding: ${paddingY / 2}vh 0 !important;
            `
            : `
                overflow-y: auto !important;
                height: ${100 - paddingY}vh !important;
                margin: ${paddingY / 2}vh 0 !important;
                padding: 0 ${paddingX / 2}vw !important;
            `;

        let gridCss = '';
        if (!layout.justify) {
            gridCss = `
                text-align: start !important;
                hanging-punctuation: allow-end !important;
                line-break: strict !important;
            `;
        }

        let fontSizeCss = '';
        if (layout.fontSize) {
            fontSizeCss = `font-size: ${layout.fontSize}px !important;`;
        }

        let textSpacingCss = '';
        let paragraphSpacingCss = '';
        if (layout.advanced) {
            const sides = vertical ? ['right', 'left'] : ['top', 'bottom'];

            textSpacingCss = `
                line-height: ${layout.lineHeight} !important;
                letter-spacing: ${layout.charSpacing / 100}em !important;
            `;
            paragraphSpacingCss = `
                p {
                    margin-${sides[0]}: ${layout.paraSpacing}em !important;
                    margin-${sides[1]}: ${layout.paraSpacing}em !important;
                }
            `;
        }

        let dimmedFuriganaCss = '';
        if (furiganaMode === 'Dimmed') {
            dimmedFuriganaCss = `
                ruby > rt, ruby > rp {
                    opacity: 0.4 !important;
                }
            `;
        }

        style.textContent = `
            :root {
                ${contentSizeCss}
            }
            @media (prefers-color-scheme: light) {
                :root {
                    --hoshi-text-color: #000;
                }
            }
            @media (prefers-color-scheme: dark) {
                :root {
                    --hoshi-text-color: #fff;
                }
            }
            html {
                -webkit-line-box-contain: block glyphs replaced;
            }
            html, body, hoshi-html, hoshi-body {
                max-height: none !important;
                max-width: none !important;
                margin: 0 !important;
                padding: 0 !important;
                color: var(--hoshi-text-color) !important;
                writing-mode: ${writingMode} !important;
            }
            html, body {
                overflow: hidden !important;
                height: 100vh !important;
                width: 100vw !important;
            }
            body {
                box-sizing: border-box !important;
                ${scrollCss}
                overflow-anchor: none !important;
                transform: translate(0) !important;
                scrollbar-width: none !important;
            }
            body::-webkit-scrollbar {
                display: none !important;
            }
            hoshi-section {
                display: flow-root !important;
            }
            hoshi-html, hoshi-body {
                display: block !important;
                height: auto !important;
                width: auto !important;
            }
            hoshi-html {
                background: transparent !important;
            }
            body, hoshi-body {
                -webkit-text-size-adjust: none !important;
                font-family: ${fontFamily} !important;
                ${gridCss}
                ${textSpacingCss}
                ${fontSizeCss}
            }
            ${paragraphSpacingCss}
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
            ${dimmedFuriganaCss}
            ruby.furigana-hidden > rp,
            ruby.furigana-hidden > rt > * {
                visibility: hidden !important;
            }
            ruby.furigana-hidden {
                cursor: pointer;
            }
            ruby.furigana-hidden > rt {
                color: transparent !important;
                background-image: linear-gradient(${furiganaLine}, ${furiganaLine}) !important;
                background-size: ${vertical ? '0.14em 100%' : '100% 0.14em'} !important;
                background-position: ${vertical ? 'left center' : 'center bottom'} !important;
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
            .hoshi-highlight-yellow {
                background-color: rgba(239, 209, 56, 0.35) !important;
            }
            .hoshi-highlight-green {
                background-color: rgba(152, 220, 129, 0.35) !important;
            }
            .hoshi-highlight-blue {
                background-color: rgba(149, 185, 255, 0.35) !important;
            }
            .hoshi-highlight-pink {
                background-color: rgba(255, 155, 180, 0.35) !important;
            }
            .hoshi-highlight-purple {
                background-color: rgba(197, 175, 251, 0.35) !important;
            }
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

    function applyFont() {
        if (!fontName || !fontFile) {
            return;
        }

        const fontStyle = document.createElement('style');
        fontStyle.textContent = `
            @font-face {
                font-family: "${fontName}";
                src: url("/__hoshi/Fonts/${encodeURIComponent(fontFile)}");
            }
        `;
        document.head.appendChild(fontStyle);
    }

    function applyColors() {
        if (sasayakiTextColor) {
            root.style.setProperty('--hoshi-sasayaki-text-color', sasayakiTextColor);
        }
        if (sasayakiBackgroundColor) {
            root.style.setProperty('--hoshi-sasayaki-background-color', sasayakiBackgroundColor);
        }
        if (textColor) {
            root.style.setProperty('--hoshi-text-color', '#' + textColor);
        }
    }

    function applyFurigana() {
        if (furiganaMode === 'Toggle') {
            document.querySelectorAll('ruby').forEach(ruby => {
                if (ruby.querySelector('rt')) {
                    ruby.classList.add('furigana-hidden');
                }
            });
        } else if (furiganaMode === 'Hidden') {
            document.querySelectorAll('rt').forEach(rt => rt.remove());
        }
    }

    function toBookOffsets(chapters, field, sectionStart) {
        return chapters.flatMap((items, spine) => {
            const start = reader.sections[spine][sectionStart];
            return items.map(item => ({ ...item, [field]: item[field] + start }));
        });
    }

    function applyCues(cues) {
        if (cues) {
            reader.applySasayakiCues(toBookOffsets(cues, 'start', 'start'));
        }
    }

    function applyHighlights(highlights) {
        if (highlights) {
            window.hoshiHighlights.applyHighlights(toBookOffsets(highlights, 'offset', 'rawStart'));
        }
    }

    function createHighlight(color, id) {
        const selection = window.getSelection();
        const section = reader.sectionOf(input.highlightRange.startContainer);

        selection.removeAllRanges();
        selection.addRange(input.highlightRange);
        input.highlightRange = null;

        const result = window.hoshiHighlights.createHighlight(color, id);
        if (!result) {
            return;
        }

        if (!result.id) {
            result.spine = section.index;
            result.start -= section.start;
            result.offset -= section.rawStart;
        }
        post({ hoshi: 'highlight-created', id, color, result });
    }

    function showSearchHighlight(spine, offset, length) {
        const section = reader.sections[spine];
        window.hoshiHighlights.showSearchHighlight(offset + section.start, length);
    }

    function onTextSelected(data) {
        const section = reader.sectionOf(window.hoshiSelection.selection.startNode);

        post({
            hoshi: 'selected',
            ...data,
            spine: section.index,
            normalizedOffset: data.normalizedOffset - section.start
        });
    }

    function report(hoshi, jump) {
        position = reader.calculateProgress();
        post({ hoshi, ...position, jump });
    }

    async function settle(move) {
        moving++;
        await move?.();
        await new Promise(resolve => {
            requestAnimationFrame(() => requestAnimationFrame(resolve));
        });
        moving--;
    }

    function revealed() {
        settle();
        report('progress');
    }

    function positioned() {
        root.style.transition = 'opacity 200ms ease';
        root.style.opacity = '1';
        post({ hoshi: 'positioned' });
    }

    function restore(spine, progress) {
        restored = true;
        position = { spine, frac: progress };
        settle(() => reader.restoreProgress(spine, progress)).then(positioned);
    }

    function jumpToFragment(spine, fragment) {
        restored = true;
        settle(() => reader.jumpToFragment(spine, fragment)).then(() => {
            report('progress', true);
            positioned();
        });
    }

    function reflow() {
        if (restored) {
            settle(() => reader.restoreProgress(position.spine, position.frac));
        }
    }

    function restyle(m) {
        layout.fontSize = m.fs;
        layout.horizontalPadding = m.hp;
        layout.verticalPadding = m.vp;
        layout.maxWidth = m.mw;
        layout.maxHeight = m.mh;
        layout.justify = m.j;
        layout.advanced = m.adv;
        layout.lineHeight = m.lh;
        layout.charSpacing = m.cs;
        layout.paraSpacing = m.ps;

        applyStyle();
        reflow();
    }

    function turn(direction) {
        const viewSize = vertical ? document.body.clientWidth : document.body.clientHeight;
        const step = viewSize * 0.8 * (direction === 'forward' ? 1 : -1);

        window.hoshiHighlights.clearSearchHighlight();
        document.body.scrollBy({
            left: vertical ? -step : 0,
            top: vertical ? 0 : step,
            behavior: 'smooth'
        });
    }

    function wheel(dx, dy) {
        if (wheelDisabled) {
            return;
        }

        const sideways = vertical && Math.abs(dx) > Math.abs(dy);
        reader.scrollBy(sideways ? -dx : dy);
    }

    function setTextColor(color) {
        if (color) {
            root.style.setProperty('--hoshi-text-color', color);
        } else {
            root.style.removeProperty('--hoshi-text-color');
        }
    }

    function scrollToSasayakiImage(spine, index) {
        const scrolled = reader.scrollToSasayakiImage(spine, index);
        if (scrolled) {
            revealed();
        }
        post({ hoshi: 'sasayaki-image-result', paused: scrolled !== null });
    }

    function onScroll() {
        if (!restored || moving) {
            return;
        }

        const now = performance.now();
        if (now - scrollReported >= 50) {
            scrollReported = now;
            report('scroll');
        }

        clearTimeout(scrollTimer);
        scrollTimer = setTimeout(() => report('progress'), 150);
    }

    function onResize() {
        if (layout.maxWidth || layout.maxHeight) {
            applyStyle();
        }
        reflow();
    }

    function onWheel(e) {
        if (!vertical && !wheelDisabled) {
            return;
        }

        e.preventDefault();
        wheel(e.deltaX, e.deltaY);
    }

    function onMessage(e) {
        const m = e.data;

        switch (m?.hoshi) {
            case 'reader-hotkeys':
                input.readerHotkeys = m.keys;
                break;
            case 'turn':
                turn(m.dir);
                break;
            case 'wheel':
                wheel(m.dx, m.dy);
                break;
            case 'restore':
                applyCues(m.cues);
                applyHighlights(m.highlights);
                restore(m.spine, m.progress);
                break;
            case 'fragment':
                applyCues(m.cues);
                applyHighlights(m.highlights);
                jumpToFragment(m.spine, m.fragment);
                break;
            case 'restyle':
                restyle(m);
                break;
            case 'create-highlight':
                createHighlight(m.color, m.id);
                break;
            case 'remove-highlight':
                window.hoshiHighlights.removeHighlight(m.id);
                break;
            case 'search-highlight':
                showSearchHighlight(m.spine, m.offset, m.length);
                break;
            case 'highlight':
                window.hoshiSelection.highlightSelection(m.count);
                break;
            case 'textcolor':
                setTextColor(m.tc);
                break;
            case 'clear-selection':
                window.hoshiSelection.clearSelection();
                break;
            case 'sasayaki-colors':
                root.style.setProperty('--hoshi-sasayaki-text-color', m.text);
                root.style.setProperty('--hoshi-sasayaki-background-color', m.background);
                break;
            case 'sasayaki-highlight':
                if (reader.highlightSasayakiCue(m.id, m.reveal)) {
                    revealed();
                }
                break;
            case 'sasayaki-clear':
                reader.clearSasayakiCue();
                break;
            case 'sasayaki-cues':
                applyCues(m.cues);
                break;
            case 'sasayaki-image':
                scrollToSasayakiImage(m.spine, m.index);
                break;
        }
    }

    async function load() {
        await reader.loadSections(window.hoshiSpine);
        applyFurigana();
        await input.setupImages();

        reader.measureSections();
        post({ hoshi: 'ready' });
    }

    window.webkit = {
        messageHandlers: {
            textSelected: { postMessage: onTextSelected }
        }
    };

    applyColors();
    applyFont();
    document.head.appendChild(style);
    applyStyle();
    reader.registerCopyText();

    document.body.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', onResize);
    window.addEventListener('wheel', onWheel, { passive: false });
    window.addEventListener('message', onMessage);

    load();
})();
