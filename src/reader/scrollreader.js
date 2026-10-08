//
//  scrollreader.js
//  Hoshi Reader
//
//  Copyright © 2026 Manhhao.
//  SPDX-License-Identifier: GPL-3.0-or-later
//

window.hoshiReader = {
    ttuRegexNegated: /[^0-9A-Za-z○◯々-〇〻ぁ-ゖゝ-ゞァ-ヺー０-９Ａ-Ｚａ-ｚｦ-ﾝ가-힣ㄱ-ㆎ\p{Radical}\p{Unified_Ideograph}]+/gimu,
    ttuRegex: /[0-9A-Za-z○◯々-〇〻ぁ-ゖゝ-ゞァ-ヺー０-９Ａ-Ｚａ-ｚｦ-ﾝ가-힣ㄱ-ㆎ\p{Radical}\p{Unified_Ideograph}]/iu,
    activeCueId: null,
    cueWrappers: new Map(),
    nodeStartOffsets: new WeakMap(),
    nodeStartRawOffsets: new WeakMap(),
    sections: [],

    isVertical() {
        return window.getComputedStyle(document.body).writingMode === "vertical-rl";
    },

    isFurigana(node) {
        const el = node.nodeType === Node.TEXT_NODE ? node.parentElement : node;
        return !!el?.closest('rt, rp');
    },

    countChars(text) {
        return Array.from(this.normalizeText(text)).length;
    },

    countRawChars(text) {
        return Array.from(text).length;
    },

    normalizeText(text) {
        return text.replace(this.ttuRegexNegated, '');
    },

    isMatchableChar(char) {
        return this.ttuRegex.test(char || '');
    },

    createWalker(rootNode) {
        const root = rootNode || document.body;

        return document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
            acceptNode: (n) => this.isFurigana(n) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT
        });
    },

    getRect(target) {
        const rect = target.getClientRects()[0];
        return rect || target.getBoundingClientRect();
    },

    async awaitFonts() {
        const style = window.getComputedStyle(document.body);
        try {
            await document.fonts.load(`${style.fontSize} ${style.fontFamily}`, 'あ');
        } catch {}
        await document.fonts.ready;
    },

    async loadChapter(href) {
        const url = new URL(encodeURI(href), location.href);
        const response = await fetch(`${url}?shell=1`);
        const text = await response.text();

        let doc = new DOMParser().parseFromString(text, 'application/xhtml+xml');
        if (!doc.body || doc.querySelector('parsererror')) {
            doc = new DOMParser().parseFromString(text, 'text/html');
        }

        return { url, doc };
    },

    copyAttributes(source, target) {
        for (const attribute of source.attributes) {
            target.setAttributeNS(attribute.namespaceURI, attribute.name, attribute.value);
        }
    },

    resolveUrls(body, url) {
        const xlink = 'http://www.w3.org/1999/xlink';

        body.querySelectorAll('[src], [href], [*|href]').forEach(el => {
            for (const name of ['src', 'href']) {
                const value = el.getAttribute(name);
                if (value) {
                    el.setAttribute(name, new URL(value, url).href);
                }
            }

            const linked = el.getAttributeNS(xlink, 'href');
            if (linked) {
                el.setAttributeNS(xlink, 'xlink:href', new URL(linked, url).href);
            }
        });
    },

    collectStyles(doc, url, sheets) {
        const tokens = [];
        const selector = 'style, link[rel~="stylesheet" i]:not([rel~="alternate" i])';

        doc.querySelectorAll(selector).forEach(node => {
            const media = node.getAttribute('media') || '';
            const source = node.localName === 'style'
                ? `${url}?css=${encodeURIComponent(node.textContent)}`
                : new URL(node.getAttribute('href'), url).href;
            const key = `${media}\n${source}`;

            if (!sheets.has(key)) {
                sheets.set(key, { token: `s${sheets.size}`, media, source });
            }

            tokens.push(sheets.get(key).token);
            node.remove();
        });

        return tokens;
    },

    createStyleLink(sheet) {
        const link = document.createElement('link');
        const separator = sheet.source.includes('?') ? '&' : '?';

        link.rel = 'stylesheet';
        link.media = sheet.media;
        link.href = `${sheet.source}${separator}continuous=${sheet.token}`;

        return link;
    },

    createSection(index, chapter, sheets) {
        const { url, doc } = chapter;
        const el = document.createElement('hoshi-section');
        const html = document.createElement('hoshi-html');
        const body = document.createElement('hoshi-body');

        this.copyAttributes(doc.documentElement, html);
        this.copyAttributes(doc.body, body);
        html.classList.add('hoshi-root');
        el.dataset.hoshiStyles = this.collectStyles(doc, url, sheets).join(' ');

        doc.body.querySelectorAll('script').forEach(node => node.remove());
        this.resolveUrls(doc.body, url);

        body.append(...doc.body.childNodes);
        html.append(body);
        el.append(html);

        return { index, el, body, start: 0, rawStart: 0, total: 0 };
    },

    async loadSections(spine) {
        const chapters = await Promise.all(spine.map(href => this.loadChapter(href)));
        const sheets = new Map();

        this.sections = chapters.map((chapter, index) => this.createSection(index, chapter, sheets));

        const links = Array.from(sheets.values(), sheet => this.createStyleLink(sheet));
        const loads = links.map(link => new Promise(resolve => {
            link.onload = resolve;
            link.onerror = resolve;
        }));

        document.head.prepend(...links);
        document.body.replaceChildren(...this.sections.map(section => section.el));
        await Promise.all(loads);
    },

    measureSections() {
        let count = 0;
        let rawCount = 0;

        for (const section of this.sections) {
            const walker = this.createWalker(section.body);
            let node;

            section.start = count;
            section.rawStart = rawCount;

            while (node = walker.nextNode()) {
                count += this.countChars(node.textContent);
                rawCount += this.countRawChars(node.textContent);
            }

            section.total = count - section.start;
        }

        this.buildNodeOffsets();
    },

    sectionOf(node) {
        const el = node.nodeType === Node.TEXT_NODE ? node.parentElement : node;
        const sectionEl = el.closest('hoshi-section');

        return this.sections.find(section => section.el === sectionEl);
    },

    getViewport() {
        const box = document.body.getBoundingClientRect();

        if (this.isVertical()) {
            return {
                size: box.width,
                start: rect => box.right - rect.right,
                end: rect => box.right - rect.left
            };
        }
        return {
            size: box.height,
            start: rect => rect.top - box.top,
            end: rect => rect.bottom - box.top
        };
    },

    scrollBy(distance) {
        if (this.isVertical()) {
            document.body.scrollLeft -= distance;
        } else {
            document.body.scrollTop += distance;
        }
    },

    scrollToTarget(target) {
        const viewport = this.getViewport();
        const rect = this.getRect(target);

        if (viewport.start(rect) >= 0 && viewport.end(rect) <= viewport.size) {
            return false;
        }

        this.scrollBy(viewport.start(rect));
        return true;
    },

    buildNodeOffsets() {
        const offsets = new WeakMap();
        const rawOffsets = new WeakMap();
        const walker = this.createWalker();
        let count = 0;
        let rawCount = 0;
        let node;

        while (node = walker.nextNode()) {
            offsets.set(node, count);
            rawOffsets.set(node, rawCount);
            count += this.countChars(node.textContent);
            rawCount += this.countRawChars(node.textContent);
        }

        this.nodeStartOffsets = offsets;
        this.nodeStartRawOffsets = rawOffsets;
    },

    calculateProgress() {
        var viewport = this.getViewport();
        var section = this.sections[0];
        for (const candidate of this.sections) {
            if (viewport.start(candidate.el.getBoundingClientRect()) <= 1) {
                section = candidate;
            }
        }

        var walker = this.createWalker(section.body);
        var range = document.createRange();
        var exploredChars = 0;
        var node;

        while (node = walker.nextNode()) {
            var nodeLen = this.countChars(node.textContent);

            if (nodeLen > 0) {
                range.selectNodeContents(node);
                if (viewport.end(range.getBoundingClientRect()) < 0) {
                    exploredChars += nodeLen;
                }
            }
        }

        return { spine: section.index, frac: section.total > 0 ? exploredChars / section.total : 0 };
    },

    collectSasayakiCueRanges(cues) {
        const cueRanges = new Map();
        if (!cues.length) {
            return [];
        }

        let index = 0;
        let current = cues[0];
        let start = current.start;
        let end = start + current.length;
        let cursor = 0;
        let segment = null;

        const flushSegment = (node) => {
            if (!segment) {
                return;
            }

            const ranges = cueRanges.get(segment.id) || [];
            ranges.push({ node, start: segment.start, end: segment.end });
            cueRanges.set(segment.id, ranges);
            segment = null;
        };

        const advanceCue = () => {
            index += 1;
            current = cues[index];
            if (current) {
                start = current.start;
                end = start + current.length;
            }
        };

        let node;
        const walker = this.createWalker();
        while (current && (node = walker.nextNode())) {
            const text = node.textContent;
            let i = 0;
            while (i < text.length && current) {
                const char = String.fromCodePoint(text.codePointAt(i));
                const next = i + char.length;
                if (this.isMatchableChar(char)) {
                    if (cursor >= start && cursor < end) {
                        if (!segment) {
                            segment = { id: current.id, start: i, end: next };
                        } else {
                            segment.end = next;
                        }
                    } else {
                        flushSegment(node);
                    }
                    cursor += 1;
                    if (cursor === end) {
                        flushSegment(node);
                        advanceCue();
                    }
                } else if (segment) {
                    segment.end = next;
                } else if (cursor > start && cursor < end) {
                    segment = { id: current.id, start: i, end: next };
                }
                i = next;
            }
            flushSegment(node);
        }

        return cues.map(cue => ({
            id: cue.id,
            ranges: cueRanges.get(cue.id) || []
        }));
    },

    applySasayakiCues(cues) {
        this.resetSasayakiCues();

        const cueRanges = this.collectSasayakiCueRanges(cues);
        const range = document.createRange();
        for (let i = cueRanges.length - 1; i >= 0; i--) {
            const { id, ranges } = cueRanges[i];
            if (!ranges.length) {
                continue;
            }

            const wrappers = [];
            for (let j = ranges.length - 1; j >= 0; j--) {
                const segment = ranges[j];
                range.setStart(segment.node, segment.start);
                range.setEnd(segment.node, segment.end);

                const wrapper = document.createElement('span');
                wrapper.className = 'hoshi-sasayaki-cue';
                wrapper.appendChild(range.extractContents());
                range.insertNode(wrapper);

                wrappers.push(wrapper);
            }
            wrappers.reverse();
            this.cueWrappers.set(id, wrappers);
        }

        this.buildNodeOffsets();
    },

    highlightSasayakiCue(cueId, reveal) {
        this.clearSasayakiCue();

        const wrappers = this.cueWrappers.get(cueId);
        if (!wrappers?.length) {
            return false;
        }

        this.activeCueId = cueId;
        wrappers.forEach(wrapper => wrapper.classList.add('hoshi-sasayaki-active'));

        return reveal && this.scrollToTarget(wrappers[0]);
    },

    clearSasayakiCue() {
        if (!this.activeCueId) {
            return;
        }

        const wrappers = this.cueWrappers.get(this.activeCueId) || [];
        wrappers.forEach(wrapper => wrapper.classList.remove('hoshi-sasayaki-active'));
        this.activeCueId = null;
    },

    scrollToSasayakiImage(spine, index) {
        const el = this.sections[spine].body.querySelectorAll('img, image')[index];
        if (!el || !(el.classList.contains('block-img') || el.namespaceURI === 'http://www.w3.org/2000/svg')) {
            return null;
        }

        return this.scrollToTarget(el);
    },

    resetSasayakiCues() {
        this.cueWrappers.forEach(wrappers => this.unwrap(wrappers));
        this.cueWrappers.clear();
        this.activeCueId = null;
    },

    unwrap(wrappers) {
        wrappers.forEach(wrapper => {
            const parent = wrapper.parentNode;
            if (!parent) {
                return;
            }
            while (wrapper.firstChild) {
                parent.insertBefore(wrapper.firstChild, wrapper);
            }
            parent.removeChild(wrapper);
            parent.normalize();
        });
    },

    registerCopyText() {
        if (window.copyTextRegistered) {
            return;
        }
        window.copyTextRegistered = true
        document.addEventListener('copy', function (event) {
            const selection = window.getSelection();
            if (!selection || selection.rangeCount === 0) {
                return;
            }
            const fragment = selection.getRangeAt(0).cloneContents();
            fragment.querySelectorAll('rt, rp').forEach(el => el.remove());
            const text = fragment.textContent;
            if (!text) {
                return;
            }
            event.preventDefault();
            event.clipboardData.setData('text/plain', text);
        }, true);
    },

    async restoreProgress(spine, progress) {
        await this.awaitFonts();
        var section = this.sections[spine];
        var rect = section.el.getBoundingClientRect();

        if (progress > 0 && section.total > 0) {
            var targetCharCount = Math.ceil(section.total * progress);
            var runningSum = 0;
            var targetNode = null;
            var walker = this.createWalker(section.body);
            var node;

            while (node = walker.nextNode()) {
                runningSum += this.countChars(node.textContent);
                targetNode = node;
                if (runningSum > targetCharCount) {
                    break;
                }
            }

            var range = document.createRange();
            range.selectNodeContents(targetNode);
            rect = this.getRect(range);
        }

        this.scrollBy(this.getViewport().start(rect));
    },

    async jumpToFragment(spine, fragment) {
        await this.awaitFonts();
        var section = this.sections[spine];
        var rawFragment = CSS.escape((fragment || '').trim());
        var target = section.el;

        if (rawFragment) {
            var selector = `[id="${rawFragment}"], [name="${rawFragment}"]`;
            target = section.el.querySelector(selector) || section.el;
        }

        this.scrollBy(this.getViewport().start(this.getRect(target)));
    }
};
