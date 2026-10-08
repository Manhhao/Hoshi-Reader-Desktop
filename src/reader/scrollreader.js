//
//  scrollreader.js
//  Hoshi Reader
//
//  Copyright © 2026 Manhhao.
//  SPDX-License-Identifier: GPL-3.0-or-later
//

Object.assign(window.hoshiReader, {
    sections: [],

    async loadChapter(href) {
        const url = new URL(encodeURI(href), location.href);
        const response = await fetch(`${url}?shell=1`);
        const text = await response.text();
        const doc = new DOMParser().parseFromString(text, 'application/xhtml+xml');

        return { url, doc };
    },

    copyAttributes(source, target) {
        for (const attribute of source.attributes) {
            target.setAttributeNS(attribute.namespaceURI, attribute.name, attribute.value);
        }
    },

    resolveUrls(body, url) {
        const xlink = 'http://www.w3.org/1999/xlink';

        body.querySelectorAll('[src], [*|href]').forEach(el => {
            for (const name of ['src', 'href']) {
                const value = el.getAttribute(name);
                if (value && URL.canParse(value, url)) {
                    el.setAttribute(name, new URL(value, url).href);
                }
            }

            const linked = el.getAttributeNS(xlink, 'href');
            if (linked && URL.canParse(linked, url)) {
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

    scrollToSasayakiImage(spine, index) {
        const el = this.sections[spine].body.querySelectorAll('img, image')[index];
        if (!el || !(el.classList.contains('block-img') || el.namespaceURI === 'http://www.w3.org/2000/svg')) {
            return null;
        }

        return this.scrollToTarget(el);
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
        var rawFragment = CSS.escape(fragment.trim());
        var target = section.el.querySelector(`[id="${rawFragment}"], [name="${rawFragment}"]`);

        this.scrollBy(this.getViewport().start(this.getRect(target || section.el)));
    }
});
