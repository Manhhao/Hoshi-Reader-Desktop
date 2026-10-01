//
//  continuous.js
//  Hoshi Reader
//
//  Copyright © 2026 Manhhao.
//  SPDX-License-Identifier: GPL-3.0-or-later
//

window.hoshiContinuous = {
    spine: [],
    sections: new Map(),
    lo: 0,
    hi: -1,
    active: 0,
    anchor: null,
    ownPos: -1,
    anchorNear: 0,
    last: null,
    root: "",
    vertical: false,
    loadDistance: 3,
    evictDistance: 8,
    maxSteps: 24,
    retryDelay: 3000,
    pumping: false,
    dirty: false,
    ticking: false,
    reportTimer: 0,
    observer: null,
    styles: new Set(),
    prepare: null,
    onProgress: null,
    onLoad: null,

    init(spine, index) {
        const reader = window.hoshiReader;
        this.spine = spine;
        this.vertical = reader.isVertical();
        this.root = `${location.protocol}//${location.host}/${location.pathname.split('/')[1]}/`;
        this.observer = new ResizeObserver(entries => this.onResize(entries));

        const section = this.createSection(index);
        [...document.body.childNodes]
            .filter(node => node.nodeName.toLowerCase() !== 'script')
            .forEach(node => section.el.appendChild(node));
        document.body.prepend(section.el);
        this.lo = this.hi = index;
        this.active = index;
        this.adopt(section);
        this.index(section);
        section.loaded = true;

        window.addEventListener('scroll', () => this.onScroll(), { passive: true, capture: true });
    },

    url(i) {
        return this.root + encodeURI(this.spine[i]);
    },

    near(rect) {
        const box = document.body.getBoundingClientRect();
        return this.vertical ? box.right - rect.right : rect.top - box.top;
    },
    
    far(rect) {
        const box = document.body.getBoundingClientRect();
        return this.vertical ? box.right - rect.left : rect.bottom - box.top;
    },
    
    viewSize() {
        return this.vertical ? document.body.clientWidth : document.body.clientHeight;
    },
    
    getPos() {
        return this.vertical ? -document.body.scrollLeft : document.body.scrollTop;
    },

    setPos(pos) {
        const value = Math.max(0, pos);
        if (this.vertical) {
            document.body.scrollLeft = -value;
        } else {
            document.body.scrollTop = value;
        }
    },

    blockSize(el) {
        const rect = el.getBoundingClientRect();
        return this.vertical ? rect.width : rect.height;
    },

    createSection(i) {
        const el = document.createElement('hoshi-section');
        el.className = 'hoshi-section';
        el.dataset.spine = i;
        const section = { i, el, loaded: false, loading: false, failedAt: 0, token: 0, nodes: [], total: 0, size: 0 };
        el.hoshiSection = section;
        this.sections.set(i, section);
        return section;
    },

    adopt(section) {
        section.size = this.blockSize(section.el);
        this.observer.observe(section.el);
    },

    index(section) {
        const result = window.hoshiReader.buildNodeOffsets(section.el);
        section.nodes = result.nodes;
        section.total = result.total;
    },

    spineOf(node) {
        const el = node?.nodeType === Node.ELEMENT_NODE ? node : node?.parentElement;
        return el?.closest('hoshi-section')?.hoshiSection?.i ?? null;
    },

    setAnchor(el) {
        this.anchor = el;
        this.anchorNear = this.near(el.getBoundingClientRect());
    },
    
    pickAnchor() {
        const box = document.body.getBoundingClientRect();
        const x = this.vertical ? box.right - 2 : box.left + box.width / 2;
        const y = this.vertical ? box.top + box.height / 2 : box.top + 2;
        let el = document.elementFromPoint(x, y);
        if (!el || el === document.body || !el.closest('hoshi-section')) {
            el = this.sections.get(this.active)?.el;
        }
        if (el) {
            this.setAnchor(el);
        }
    },
    
    onResize(entries) {
        for (const entry of entries) {
            const section = entry.target.hoshiSection;
            if (section) {
                section.size = entry.borderBoxSize[0].blockSize;
            }
        }
        if (!this.anchor || !this.anchor.isConnected) {
            return;
        }
        const shift = this.near(this.anchor.getBoundingClientRect()) - this.anchorNear;
        if (Math.abs(shift) > 0.5) {
            this.setPos(this.getPos() + shift);
            this.ownPos = this.getPos();
        }
    },
    
    parse(text) {
        const doc = new DOMParser().parseFromString(text, 'application/xhtml+xml');
        if (doc.getElementsByTagName('parsererror').length) {
            return new DOMParser().parseFromString(text, 'text/html');
        }
        return doc;
    },

    hoistHead(doc, base) {
        doc.querySelectorAll('link[rel~="stylesheet"], style:not([data-hoshi])').forEach(node => {
            let key;
            if (node.localName === 'link') {
                key = new URL(node.getAttribute('href'), base).href;
                if (this.styles.has(key) || [...document.head.querySelectorAll('link')].some(link => link.href === key)) {
                    this.styles.add(key);
                    return;
                }
            } else {
                key = node.textContent;
                if (this.styles.has(key)) {
                    return;
                }
            }
            this.styles.add(key);
            const copy = document.importNode(node, true);
            if (copy.localName === 'link') {
                copy.setAttribute('href', key);
            }
            document.head.appendChild(copy);
        });
    },

    absolutize(body, base) {
        const xlink = 'http://www.w3.org/1999/xlink';
        body.querySelectorAll('[src], [href], [*|href]').forEach(el => {
            for (const name of ['src', 'href']) {
                const value = el.getAttribute(name);
                if (value) {
                    el.setAttribute(name, new URL(value, base).href);
                }
            }
            const linked = el.getAttributeNS(xlink, 'href');
            if (linked) {
                el.setAttributeNS(xlink, 'xlink:href', new URL(linked, base).href);
            }
        });
    },

    async load(section) {
        if (section.loaded || section.loading) {
            return;
        }
        section.loading = true;
        const token = ++section.token;
        try {
            const url = this.url(section.i);
            const response = await fetch(url);
            if (!response.ok) {
                throw new Error(response.statusText);
            }
            const doc = this.parse(await response.text());
            if (token !== section.token) {
                return;
            }
            this.hoistHead(doc, url);
            this.absolutize(doc.body, url);
            doc.body.querySelectorAll('script').forEach(node => node.remove());
            if (doc.body.className) {
                section.el.className = `hoshi-section ${doc.body.className}`;
            }
            section.el.append(...doc.body.childNodes);
            section.el.style.blockSize = '';
            if (this.prepare) {
                await this.prepare(section.el);
            }
            if (token !== section.token) {
                return;
            }
            this.index(section);
            section.loaded = true;
            section.failedAt = 0;
            this.onLoad?.(section.i);
        } catch {
            section.el.replaceChildren();
            section.failedAt = performance.now();
        } finally {
            section.loading = false;
        }
    },

    evict(section) {
        window.hoshiHighlights.drop(section.el);
        window.hoshiReader.dropCues(section.el);
        section.token++;
        section.loading = false;
        const size = this.blockSize(section.el);
        section.el.replaceChildren();
        section.el.style.blockSize = `${size}px`;
        section.size = size;
        section.nodes = [];
        section.total = 0;
        section.loaded = false;
    },

    reset() {
        this.sections.forEach(section => {
            window.hoshiHighlights.drop(section.el);
            window.hoshiReader.dropCues(section.el);
            section.token++;
            this.observer.unobserve(section.el);
            section.el.remove();
        });
        this.sections.clear();
        this.anchor = null;
        this.lo = 0;
        this.hi = -1;
    },

    distance(section, view) {
        const rect = section.el.getBoundingClientRect();
        const near = this.near(rect);
        const far = this.far(rect);
        if (near > view) {
            return near - view;
        }
        if (far < 0) {
            return -far;
        }
        return 0;
    },

    failed(section) {
        return section.failedAt > 0 && performance.now() - section.failedAt < this.retryDelay;
    },

    findActive() {
        let a = this.lo;
        let b = this.hi;
        while (a < b) {
            const m = (a + b + 1) >> 1;
            if (this.near(this.sections.get(m).el.getBoundingClientRect()) <= 1) {
                a = m;
            } else {
                b = m - 1;
            }
        }
        return a;
    },

    async pump() {
        if (this.pumping) {
            this.dirty = true;
            return;
        }
        this.pumping = true;
        try {
            let steps = 0;
            do {
                this.dirty = false;
                this.active = this.findActive();
                const view = this.viewSize();
                const limit = this.loadDistance * view;

                this.sections.forEach(section => {
                    if (section.loaded && section.i !== this.active && this.distance(section, view) > this.evictDistance * view) {
                        this.evict(section);
                    }
                });

                const from = Math.max(this.lo, this.active - 6);
                const to = Math.min(this.hi, this.active + 6);
                for (let i = from; i <= to; i++) {
                    const section = this.sections.get(i);
                    if (section && !section.loaded && !section.loading && !this.failed(section) && this.distance(section, view) < limit) {
                        await this.load(section);
                        this.dirty = true;
                    }
                }

                const end = this.sections.get(this.hi);
                if (end && this.hi < this.spine.length - 1 && this.far(end.el.getBoundingClientRect()) - view < limit) {
                    const next = this.createSection(++this.hi);
                    document.body.insertBefore(next.el, end.el.nextSibling);
                    this.adopt(next);
                    await this.load(next);
                    this.dirty = true;
                }

                const start = this.sections.get(this.lo);
                if (start && this.lo > 0 && -this.near(start.el.getBoundingClientRect()) < limit) {
                    const previous = this.createSection(--this.lo);
                    document.body.insertBefore(previous.el, start.el);
                    this.adopt(previous);
                    await this.load(previous);
                    this.dirty = true;
                }
            } while (this.dirty && ++steps < this.maxSteps);
        } finally {
            this.pumping = false;
        }
    },

    charRect(node, offset) {
        const text = node.textContent;
        const range = document.createRange();
        const limit = Math.min(text.length, offset + 8);
        for (let i = offset; i < limit; i++) {
            range.setStart(node, i);
            range.setEnd(node, i + 1);
            const rect = range.getClientRects()[0];
            if (rect && (rect.width || rect.height)) {
                return rect;
            }
        }
        return null;
    },

    isPast(node, offset) {
        const rect = this.charRect(node, offset);
        return rect ? this.far(rect) <= 1 : false;
    },

    exploredChars(section) {
        const nodes = section.nodes;
        let a = 0;
        let b = nodes.length;
        while (a < b) {
            const m = (a + b) >> 1;
            if (this.isPast(nodes[m].node, 0)) {
                a = m + 1;
            } else {
                b = m;
            }
        }
        if (a === 0) {
            return 0;
        }

        const { node, start } = nodes[a - 1];
        const text = node.textContent;
        let from = 0;
        let to = text.length;
        while (from < to) {
            const m = (from + to) >> 1;
            if (this.isPast(node, m)) {
                from = m + 1;
            } else {
                to = m;
            }
        }
        return start + window.hoshiReader.countChars(text.slice(0, from));
    },

    progress() {
        const section = this.sections.get(this.active);
        if (!section?.loaded) {
            return null;
        }
        if (section.total <= 0) {
            return { spine: section.i, frac: 0 };
        }
        return { spine: section.i, frac: Math.min(1, this.exploredChars(section) / section.total) };
    },
    
    locate(section, frac) {
        const reader = window.hoshiReader;
        const target = Math.min(Math.ceil(section.total * frac), section.total - 1);
        let a = 0;
        let b = section.nodes.length - 1;
        while (a < b) {
            const m = (a + b + 1) >> 1;
            if (section.nodes[m].start <= target) {
                a = m;
            } else {
                b = m - 1;
            }
        }
        const entry = section.nodes[a];
        if (!entry) {
            return null;
        }

        const want = target - entry.start;
        const text = entry.node.textContent;
        let count = 0;
        let offset = 0;
        for (let i = 0; i < text.length;) {
            const char = String.fromCodePoint(text.codePointAt(i));
            if (reader.isMatchableChar(char)) {
                if (count === want) {
                    offset = i;
                    break;
                }
                count++;
            }
            i += char.length;
        }
        return this.charRect(entry.node, offset);
    },

    place(section, frac) {
        const view = this.viewSize();
        const rect = section.el.getBoundingClientRect();
        let delta;
        if (frac >= 0.999999 || section.total <= 0 && frac > 0) {
            delta = this.far(rect) - view;
        } else if (frac <= 0) {
            delta = section.i === 0 ? -this.getPos() : this.near(rect);
        } else {
            const target = this.locate(section, frac);
            delta = target ? this.near(target) : this.near(rect);
        }
        this.setPos(this.getPos() + delta);
    },

    placeFragment(section, fragment) {
        const name = (fragment || '').trim();
        const target = name && (section.el.querySelector(`[id="${CSS.escape(name)}"]`) || section.el.querySelector(`[name="${CSS.escape(name)}"]`));
        if (!target) {
            this.place(section, 0);
            return false;
        }
        this.setPos(this.getPos() + this.near(target.getBoundingClientRect()));
        return true;
    },

    async goTo(i, frac, fragment, rebuild) {
        const reader = window.hoshiReader;
        let section = this.sections.get(i);
        if (rebuild || !section) {
            this.reset();
            section = this.createSection(i);
            document.body.prepend(section.el);
            this.lo = this.hi = i;
            this.adopt(section);
        }
        this.active = i;
        await this.load(section);
        this.setAnchor(section.el);
        await reader.awaitFonts();
        if (fragment) {
            this.placeFragment(section, fragment);
        } else {
            this.place(section, frac);
        }
        this.active = this.findActive();
        this.pickAnchor();
        this.last = this.progress();
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        this.pump();
    },

    reveal(range) {
        const rect = window.hoshiReader.getRect(range);
        const view = this.viewSize();
        if (this.near(rect) >= 0 && this.far(rect) <= view) {
            return false;
        }
        this.setPos(this.getPos() + this.near(rect));
        return true;
    },
    
    scrollPage(direction) {
        const step = this.viewSize() * 0.8 * (direction === 'forward' ? 1 : -1);
        const target = Math.max(0, this.getPos() + step);
        document.body.scrollTo({
            [this.vertical ? 'left' : 'top']: this.vertical ? -target : target,
            behavior: 'smooth'
        });
    },

    scrollBy(dx, dy) {
        const horizontal = Math.abs(dx) > Math.abs(dy);
        const delta = this.vertical ? (horizontal ? -dx : dy) : dy;
        this.setPos(this.getPos() + delta);
    },

    onScroll() {
        if (this.anchor?.isConnected && Math.abs(this.getPos() - this.ownPos) > 1.5) {
            this.anchorNear = this.near(this.anchor.getBoundingClientRect());
        }
        if (this.ticking) {
            return;
        }
        this.ticking = true;
        requestAnimationFrame(() => {
            this.ticking = false;
            this.active = this.findActive();
            if (Math.abs(this.getPos() - this.ownPos) > 1.5) {
                this.pickAnchor();
            }
            this.last = this.progress();
            this.pump();
            if (!this.reportTimer) {
                this.reportTimer = setTimeout(() => {
                    this.reportTimer = 0;
                    this.report();
                }, 150);
            }
        });
    },

    report(jump) {
        const result = this.progress();
        if (result) {
            this.last = result;
            this.onProgress?.(result, jump);
        }
    }
};
