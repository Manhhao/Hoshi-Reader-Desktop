window.hoshiInput = (function () {
    const params = new URLSearchParams(location.search);
    const mac = navigator.userAgent.includes('Macintosh');
    const blurImages = params.get('bi') === '1';
    const scanModifier = params.get('mod');
    const scanDelay = Number(params.get('sdl'));
    const clickLookup = params.get('cl');
    const auxLookupButton = { middle: 1, back: 3, forward: 4 }[clickLookup];
    const scanButton = {
        'Mouse:Middle': 1,
        'Mouse:Right': 2,
        'Mouse:Back': 3,
        'Mouse:Forward': 4
    }[scanModifier];
    const scanButtonMask = [1, 4, 2, 8, 16][scanButton] ?? 0;
    const mouseHotkeyTokens = {
        1: 'Mouse:Middle',
        2: 'Mouse:Right',
        3: 'Mouse:Back',
        4: 'Mouse:Forward'
    };

    const input = {
        readerHotkeys: [],
        highlightRange: null,
        clickAdvance: false,
        clickZone: 0,
        setupImages
    };

    let scanKeyHeld = false;
    let scanTimer = 0;
    let lastMouse = null;
    let mouseButtons = 0;
    let mouseDownAt = null;
    let secondaryRange = null;
    let secondaryLookup = false;
    let selectionDismissed = false;
    let pressedSelection = null;

    window.scanLength = Number(params.get('sl'));
    window.scanNonJapaneseText = params.get('snj') !== '0';

    function post(message) {
        parent.postMessage(message, '*');
    }

    function isScanKey(key) {
        return (key.length === 1 ? key.toLowerCase() : key) === scanModifier;
    }

    function modifierHeld(e) {
        if (scanButtonMask) {
            return (e.buttons & scanButtonMask) !== 0;
        }
        if (scanModifier === 'Shift') {
            return e.shiftKey;
        }
        if (scanModifier === 'Control') {
            return e.ctrlKey;
        }
        if (scanModifier === 'Alt') {
            return e.altKey;
        }
        if (scanModifier === 'Meta') {
            return e.metaKey;
        }
        return scanKeyHeld;
    }

    function isSecondaryButton(e) {
        return e.button === 2 || (mac && e.ctrlKey);
    }

    function clickEdge(x) {
        if (x < window.innerWidth * input.clickZone) {
            return 'left';
        }
        if (x > window.innerWidth * (1 - input.clickZone)) {
            return 'right';
        }
        return null;
    }

    function selectText(x, y) {
        return window.hoshiSelection.selectText(x, y, window.scanLength);
    }

    function finishTextAnimation() {
        return window.hoshiParagraph?.finishTextAnimation();
    }

    function selectionRangeAt(e) {
        const selection = window.getSelection();
        if (!selection || selection.isCollapsed || window.hoshiSelection.selection) {
            return null;
        }

        const range = selection.getRangeAt(0);
        const onSelection = [...range.getClientRects()].some(rect =>
            e.clientX >= rect.left - 4 && e.clientX <= rect.right + 4 &&
            e.clientY >= rect.top - 4 && e.clientY <= rect.bottom + 4
        );

        return onSelection ? range : null;
    }

    function onContextMenu(e) {
        e.preventDefault();
        if (scanButton === 2) {
            return;
        }

        const range = clickLookup === 'right' ? secondaryRange : selectionRangeAt(e);
        if (range) {
            input.highlightRange = range.cloneRange();
            post({ hoshi: 'selection-menu' });
            return;
        }
        if (clickLookup === 'right') {
            return;
        }

        window.hoshiSelection.clearSelection();
        if (input.readerHotkeys.includes('Mouse:Right')) {
            post({ hoshi: 'reader-hotkey', key: 'Mouse:Right' });
        } else {
            post({ hoshi: 'press' });
        }
    }

    function onMouseMove(e) {
        mouseButtons = e.buttons;
        lastMouse = { x: e.clientX, y: e.clientY };
        clearTimeout(scanTimer);
        if (e.buttons & ~scanButtonMask) {
            return;
        }

        if (modifierHeld(e)) {
            if (!finishTextAnimation()) {
                selectText(e.clientX, e.clientY);
            }
            return;
        }
        if (scanModifier) {
            return;
        }

        scanTimer = setTimeout(() => {
            if (!window.hoshiParagraph?.animationFrame) {
                selectText(e.clientX, e.clientY);
            }
        }, scanDelay);
    }

    function onMouseLeave() {
        lastMouse = null;
        clearTimeout(scanTimer);
    }

    function onScanKeyDown(e) {
        if (!isScanKey(e.key)) {
            return;
        }

        scanKeyHeld = true;
        if (e.repeat || !lastMouse || mouseButtons) {
            return;
        }
        if (!finishTextAnimation()) {
            selectText(lastMouse.x, lastMouse.y);
        }
    }

    function onScanKeyUp(e) {
        if (isScanKey(e.key)) {
            scanKeyHeld = false;
        }
    }

    function onBlur() {
        scanKeyHeld = false;
        mouseButtons = 0;
    }

    function onMouseDown(e) {
        mouseButtons = e.buttons;
        clearTimeout(scanTimer);

        if (e.button === scanButton) {
            e.preventDefault();
            if (!finishTextAnimation()) {
                selectText(e.clientX, e.clientY);
            }
            return;
        }

        const token = mouseHotkeyTokens[e.button];
        const aux = e.button === auxLookupButton;
        if (token && token !== 'Mouse:Right' && !aux && input.readerHotkeys.includes(token)) {
            e.preventDefault();
            post({ hoshi: 'reader-hotkey', key: token });
            return;
        }

        const secondary = isSecondaryButton(e);
        secondaryRange = secondary ? selectionRangeAt(e) : null;

        const ignored = secondary
            ? clickLookup !== 'right' || secondaryRange
            : e.button !== 0 && !aux;
        if (ignored) {
            if (!window.getSelection().isCollapsed) {
                e.preventDefault();
            }
            return;
        }

        if (aux) {
            e.preventDefault();
        }
        secondaryLookup = secondary;
        mouseDownAt = { x: e.clientX, y: e.clientY };
        selectionDismissed = !window.getSelection().isCollapsed || !!window.hoshiSelection.selection;
        if ((input.clickAdvance || clickEdge(e.clientX)) && e.detail > 1) {
            e.preventDefault();
        }

        pressedSelection = window.hoshiSelection.selection;
        window.hoshiSelection.clearSelection();
        post({ hoshi: 'press' });
    }

    function onMouseUp(e) {
        if (e.button === scanButton) {
            return;
        }

        secondaryLookup = false;
        if (isSecondaryButton(e)) {
            if (clickLookup === 'right' && !secondaryRange) {
                onClick(e, 2);
            }
        } else if (e.button === 0 || e.button === auxLookupButton) {
            onClick(e, e.button);
        }

        setTimeout(() => post({ hoshi: 'release' }));
    }

    function onAuxClick(e) {
        const token = mouseHotkeyTokens[e.button];
        if (e.button === scanButton || e.button === auxLookupButton) {
            e.preventDefault();
        } else if (token && token !== 'Mouse:Right' && input.readerHotkeys.includes(token)) {
            e.preventDefault();
        }
    }

    function onClick(e, button) {
        if (modifierHeld(e)) {
            return;
        }

        const moved = mouseDownAt && (
            Math.abs(e.clientX - mouseDownAt.x) > 4 ||
            Math.abs(e.clientY - mouseDownAt.y) > 4
        );
        if (moved) {
            return;
        }

        const link = button === 0 && e.target instanceof Element
            ? e.target.closest('a[href]')
            : null;
        if (finishTextAnimation()) {
            return;
        }
        if (link) {
            post({ hoshi: 'link', href: link.href });
            return;
        }

        const miss = {
            hoshi: 'lookup-miss',
            edge: clickEdge(e.clientX),
            dismissed: selectionDismissed
        };
        const lookup = button !== 0 || clickLookup === 'left';
        const pointed = document.elementFromPoint(e.clientX, e.clientY);
        if (!lookup && !pointed?.closest('ruby.furigana-hidden')) {
            post(miss);
            return;
        }

        const hit = pressedSelection && window.hoshiSelection.getCharacterAtPoint(e.clientX, e.clientY);
        const repeated = hit &&
            hit.node === pressedSelection.startNode &&
            hit.offset === pressedSelection.startOffset;
        if (repeated) {
            post({ hoshi: 'lookup-miss', dismissed: true });
            return;
        }

        const selected = selectText(e.clientX, e.clientY);
        if (!selected && button === 0) {
            post(miss);
        }
    }

    function onLinkClick(e) {
        if (e.target instanceof Element && e.target.closest('a[href]')) {
            e.preventDefault();
        }
    }

    function onSelectStart(e) {
        if (secondaryLookup) {
            e.preventDefault();
        }
    }

    function onKeyDown(e) {
        if (e.defaultPrevented || e.isComposing) {
            return;
        }
        if (e.target.closest('input, textarea, select, [contenteditable]')) {
            return;
        }

        if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'f') {
            e.preventDefault();
            post({ hoshi: 'search' });
            return;
        }
        if (e.key === 'Escape') {
            e.preventDefault();
            post({ hoshi: 'escape' });
            return;
        }
        if (e.ctrlKey || e.altKey || e.metaKey) {
            return;
        }

        const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
        const control = e.target.closest('button, a, [role="button"], summary');
        const activatesControl = control && [' ', 'Enter', 'Tab'].includes(e.key);
        if (input.readerHotkeys.includes(key) && !activatesControl) {
            e.preventDefault();
            post({ hoshi: 'reader-hotkey', key, repeat: e.repeat });
        }
    }

    function setupImage(el, src, wrap, blurred = el) {
        let target = el;
        if (blurImages) {
            blurred.classList.add('blurred');
            if (wrap) {
                target = document.createElement('div');
                target.className = 'blur-wrapper';
                blurred.before(target);
                target.append(blurred);
            }
        }

        target.addEventListener('click', e => {
            e.preventDefault();
            e.stopPropagation();
            blurred.classList.remove('blurred');
        });
        target.addEventListener('dblclick', e => {
            e.preventDefault();
            e.stopPropagation();
            post({ hoshi: 'open-image', url: new URL(src, document.baseURI).href });
        });
    }

    function setupLoadedImage(img) {
        const isGaiji = img.classList.contains('gaiji') || img.classList.contains('gaiji-line');
        if (!isGaiji && (img.naturalWidth > 256 || img.naturalHeight > 256)) {
            img.classList.add('block-img');
            setupImage(img, img.src, true);
        }
    }

    function setupImages() {
        document.querySelectorAll('svg[preserveAspectRatio="none"]').forEach(svg => {
            svg.removeAttribute('preserveAspectRatio');
        });
        document.querySelectorAll('svg').forEach(svg => {
            const image = svg.querySelector('image');
            if (image) {
                setupImage(image, image.href.baseVal, false, svg);
            }
        });

        const loads = Array.from(document.querySelectorAll('img')).map(img => {
            return new Promise(resolve => {
                function processImg() {
                    setupLoadedImage(img);
                    resolve();
                }

                if (img.complete) {
                    processImg();
                } else {
                    img.onload = processImg;
                    img.onerror = () => resolve();
                }
            });
        });

        return Promise.all(loads).then(() => new Promise(resolve => setTimeout(resolve, 50)));
    }

    document.addEventListener('contextmenu', onContextMenu);
    document.addEventListener('mousemove', onMouseMove, true);
    document.addEventListener('keydown', onScanKeyDown, true);
    document.addEventListener('mouseup', e => {
        mouseButtons = e.buttons;
    }, true);
    document.addEventListener('keyup', onScanKeyUp, true);
    window.addEventListener('blur', onBlur);
    document.documentElement.addEventListener('mouseleave', onMouseLeave);
    document.addEventListener('mousedown', onMouseDown);
    document.addEventListener('selectstart', onSelectStart);
    document.addEventListener('mouseup', onMouseUp);
    document.addEventListener('auxclick', onAuxClick);
    document.addEventListener('click', onLinkClick);
    window.addEventListener('keydown', onKeyDown);

    return input;
})();
