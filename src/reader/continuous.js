window.hoshiContinuous = {
    spine: [],
    sections: new Map(),
    active: 0,
    root: '',
    vertical: false,
    reporting: false,
    ticking: false,
    reportTimer: 0,
    last: null,
    ready: Promise.resolve(),
    styles: new Map(),
    prepare: null,
    onProgress: null,
    onLoad: null,

    init(spine, index) {
        this.spine = spine;
        this.active = index;
        this.vertical = window.hoshiReader.isVertical();
        this.root = `${location.protocol}//${location.host}/${location.pathname.split('/')[1]}/`;
        this.ready = this.assemble();
        window.addEventListener('scroll', () => this.onScroll(), { passive: true, capture: true });
        return this.ready;
    },

    async assemble() {
        const documents = await Promise.all(this.spine.map(async (_, index) => {
            const response = await fetch(this.chapterUrl(index));
            if (!response.ok) throw new Error(response.statusText);
            return this.parse(await response.text());
        }));
        const styleLoads = [];
        const fragment = document.createDocumentFragment();
        for (let i = 0; i < documents.length; i++) {
            const doc = documents[i];
            const base = this.url(i);
            const styles = this.hoistHead(doc, base);
            styleLoads.push(...styles.loads);
            this.absolutize(doc.body, base);
            doc.body.querySelectorAll('script').forEach(node => node.remove());
            const section = this.createSection(i, doc, styles.tokens);
            section.body.append(...doc.body.childNodes);
            fragment.appendChild(section.el);
        }
        document.body.replaceChildren(fragment);
        await Promise.all(styleLoads);
        await Promise.all([...this.sections.values()].map(section => this.prepare?.(section.body)));
        await window.hoshiReader.awaitFonts();
        this.sections.forEach(section => this.index(section));
        this.sections.forEach(section => {
            if (section.i !== currentIndex) this.onLoad?.(section.i);
        });
    },

    url(index) {
        return this.root + encodeURI(this.spine[index]);
    },

    chapterUrl(index) {
        const url = new URL(this.url(index));
        url.searchParams.set('shell', '1');
        return url.href;
    },

    createSection(index, doc, styles) {
        const el = document.createElement('hoshi-section');
        const html = document.createElement('div');
        const body = document.createElement('div');
        el.className = 'hoshi-section';
        html.className = `hoshi-html ${doc.documentElement.className}`;
        body.className = `hoshi-body ${doc.body.className}`;
        el.dataset.spine = index;
        el.dataset.hoshiStyles = styles.join(' ');
        html.appendChild(body);
        el.appendChild(html);
        const section = { i: index, el, body, nodes: [], total: 0 };
        el.hoshiSection = section;
        body.hoshiSection = section;
        this.sections.set(index, section);
        return section;
    },

    parse(text) {
        const doc = new DOMParser().parseFromString(text, 'application/xhtml+xml');
        if (doc.getElementsByTagName('parsererror').length) {
            return new DOMParser().parseFromString(text, 'text/html');
        }
        return doc;
    },

    scopeStyle(css) {
        return css.replace(/([^{}]+)\{/gs, (rule, selector) => {
            if (selector.trimStart().startsWith('@')) return rule;
            const html = selector.replace(/(^|[\s>+~,(])(?:html|:root)($|[\s>+~.#:\[,(])/gi, '$1.hoshi-html$2');
            const body = html.replace(/(^|[\s>+~,(])body($|[\s>+~.#:\[,(])/gi, '$1.hoshi-body$2');
            return `${body}{`;
        });
    },

    hoistHead(doc, base) {
        const loads = [];
        const tokens = [];
        doc.querySelectorAll('link[rel~="stylesheet"], style:not([data-hoshi])').forEach(node => {
            if (node.localName === 'style') {
                const css = this.scopeStyle(node.textContent);
                const media = node.getAttribute('media') || '';
                const key = `inline\n${media}\n${css}`;
                let token = this.styles.get(key);
                if (!token) {
                    token = `s${this.styles.size}`;
                    this.styles.set(key, token);
                    const style = document.createElement('style');
                    style.dataset.hoshi = '';
                    style.media = media;
                    style.textContent = `@scope ([data-hoshi-styles~="${token}"]) {${css}}`;
                    document.head.appendChild(style);
                }
                if (!tokens.includes(token)) tokens.push(token);
                return;
            }
            const href = node.getAttribute('href');
            if (!href) return;
            const url = new URL(href, base);
            const rel = node.getAttribute('rel') || 'stylesheet';
            const media = node.getAttribute('media') || '';
            const key = `${rel}\n${media}\n${url.href}`;
            let token = this.styles.get(key);
            if (!token) {
                token = `s${this.styles.size}`;
                this.styles.set(key, token);
                url.searchParams.set('continuous', token);
                const link = document.createElement('link');
                link.rel = rel;
                link.media = media;
                link.href = url.href;
                loads.push(new Promise(resolve => {
                    link.addEventListener('load', resolve, { once: true });
                    link.addEventListener('error', resolve, { once: true });
                }));
                document.head.appendChild(link);
            }
            if (!tokens.includes(token)) tokens.push(token);
        });
        return { loads, tokens };
    },

    absolutize(body, base) {
        const xlink = 'http://www.w3.org/1999/xlink';
        body.querySelectorAll('[src], [href], [*|href]').forEach(el => {
            for (const name of ['src', 'href']) {
                const value = el.getAttribute(name);
                if (value) el.setAttribute(name, new URL(value, base).href);
            }
            const linked = el.getAttributeNS(xlink, 'href');
            if (linked) el.setAttributeNS(xlink, 'xlink:href', new URL(linked, base).href);
        });
    },

    index(section) {
        const result = window.hoshiReader.buildNodeOffsets(section.body);
        section.nodes = result.nodes;
        section.total = result.total;
    },

    spineOf(node) {
        const el = node?.nodeType === Node.ELEMENT_NODE ? node : node?.parentElement;
        return el?.closest('hoshi-section')?.hoshiSection?.i ?? null;
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
        if (this.vertical) document.body.scrollLeft = -value;
        else document.body.scrollTop = value;
    },

    findActive() {
        let low = 0;
        let high = this.spine.length - 1;
        while (low < high) {
            const middle = (low + high + 1) >> 1;
            if (this.near(this.sections.get(middle).el.getBoundingClientRect()) <= 1) low = middle;
            else high = middle - 1;
        }
        return low;
    },

    charRect(node, offset) {
        const range = document.createRange();
        const limit = Math.min(node.textContent.length, offset + 8);
        for (let i = offset; i < limit; i++) {
            range.setStart(node, i);
            range.setEnd(node, i + 1);
            const rect = range.getClientRects()[0];
            if (rect && (rect.width || rect.height)) return rect;
        }
        return null;
    },

    isPast(node, offset) {
        const rect = this.charRect(node, offset);
        return rect ? this.far(rect) <= 1 : false;
    },

    exploredChars(section) {
        let low = 0;
        let high = section.nodes.length;
        while (low < high) {
            const middle = (low + high) >> 1;
            if (this.isPast(section.nodes[middle].node, 0)) low = middle + 1;
            else high = middle;
        }
        if (low === 0) return 0;
        const { node, start } = section.nodes[low - 1];
        let from = 0;
        let to = node.textContent.length;
        while (from < to) {
            const middle = (from + to) >> 1;
            if (this.isPast(node, middle)) from = middle + 1;
            else to = middle;
        }
        return start + window.hoshiReader.countChars(node.textContent.slice(0, from));
    },

    progress() {
        const section = this.sections.get(this.active);
        if (!section || section.total <= 0) return { spine: this.active, frac: 0 };
        return { spine: section.i, frac: Math.min(1, this.exploredChars(section) / section.total) };
    },

    locate(section, frac) {
        const target = Math.min(Math.ceil(section.total * frac), section.total - 1);
        let low = 0;
        let high = section.nodes.length - 1;
        while (low < high) {
            const middle = (low + high + 1) >> 1;
            if (section.nodes[middle].start <= target) low = middle;
            else high = middle - 1;
        }
        const entry = section.nodes[low];
        if (!entry) return null;
        const wanted = target - entry.start;
        let count = 0;
        let offset = 0;
        for (let i = 0; i < entry.node.textContent.length;) {
            const char = String.fromCodePoint(entry.node.textContent.codePointAt(i));
            if (window.hoshiReader.isMatchableChar(char)) {
                if (count === wanted) {
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
        const rect = section.el.getBoundingClientRect();
        let delta;
        if (frac >= 0.999999 || section.total <= 0 && frac > 0) delta = this.far(rect) - this.viewSize();
        else if (frac <= 0) delta = section.i === 0 ? -this.getPos() : this.near(rect);
        else {
            const target = this.locate(section, frac);
            delta = target ? this.near(target) : this.near(rect);
        }
        this.setPos(this.getPos() + delta);
    },

    placeFragment(section, fragment) {
        const name = (fragment || '').trim();
        const target = name && (section.body.querySelector(`[id="${CSS.escape(name)}"]`) || section.body.querySelector(`[name="${CSS.escape(name)}"]`));
        if (!target) this.place(section, 0);
        else this.setPos(this.getPos() + this.near(target.getBoundingClientRect()));
    },

    async goTo(index, frac, fragment) {
        await this.ready;
        const section = this.sections.get(index);
        if (!section) return;
        this.active = index;
        await window.hoshiReader.awaitFonts();
        if (fragment) this.placeFragment(section, fragment);
        else this.place(section, frac);
        this.active = this.findActive();
        this.last = this.progress();
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    },

    async reflow(anchor) {
        await this.ready;
        await window.hoshiReader.awaitFonts();
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const section = this.sections.get(anchor.spine);
        if (!section) return;
        this.place(section, anchor.frac);
        this.active = this.findActive();
        this.report(true);
    },

    reveal(range) {
        const rect = window.hoshiReader.getRect(range);
        if (this.near(rect) >= 0 && this.far(rect) <= this.viewSize()) return false;
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
        if (!this.reporting || this.ticking || this.sections.size !== this.spine.length) return;
        this.ticking = true;
        requestAnimationFrame(() => {
            this.ticking = false;
            this.active = this.findActive();
            if (!this.reportTimer) {
                this.reportTimer = setTimeout(() => {
                    this.reportTimer = 0;
                    this.report();
                }, 50);
            }
        });
    },

    report(jump) {
        this.last = this.progress();
        this.onProgress?.(this.last, jump);
    }
};
