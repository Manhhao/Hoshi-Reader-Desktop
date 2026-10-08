//
//  reader.js
//  Hoshi Reader
//
//  Copyright © 2026 Manhhao.
//  SPDX-License-Identifier: GPL-3.0-or-later
//

window.hoshiReader = {
    ttuRegexNegated: /[^0-9A-Za-z○◯々-〇〻ぁ-ゖゝ-ゞァ-ヺー０-９Ａ-Ｚａ-ｚｦ-ﾝ가-힣ㄱ-ㆎ\p{Radical}\p{Unified_Ideograph}]+/gimu,
    ttuRegex: /[0-9A-Za-z○◯々-〇〻ぁ-ゖゝ-ゞァ-ヺー０-９Ａ-Ｚａ-ｚｦ-ﾝ가-힣ㄱ-ㆎ\p{Radical}\p{Unified_Ideograph}]/iu,
    activeCueId: null,
    spacer: null,
    pagesPerScreen: 1,
    cueWrappers: new Map(),
    nodeStartOffsets: new WeakMap(),
    nodeStartRawOffsets: new WeakMap(),
    
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

    buildNodeOffsets(root) {
        const offsets = root ? this.nodeStartOffsets : new WeakMap();
        const rawOffsets = root ? this.nodeStartRawOffsets : new WeakMap();
        const walker = this.createWalker(root);
        const nodes = [];
        let count = 0;
        let rawCount = 0;
        let node;

        while (node = walker.nextNode()) {
            const length = this.countChars(node.textContent);
            offsets.set(node, count);
            rawOffsets.set(node, rawCount);
            if (length > 0) {
                nodes.push({ node, start: count });
            }
            count += length;
            rawCount += this.countRawChars(node.textContent);
        }

        this.nodeStartOffsets = offsets;
        this.nodeStartRawOffsets = rawOffsets;
        return { nodes, total: count };
    },

    refreshOffsets(root) {
        if (root?.hoshiSection) {
            window.hoshiContinuous.index(root.hoshiSection);
        } else {
            this.buildNodeOffsets();
        }
    },

    currentProgress() {
        return window.hoshiContinuous?.sections.size ? 0 : this.calculateProgress();
    },
    
    calculateProgress() {
        var vertical = this.isVertical();
        var walker = this.createWalker();
        var totalChars = 0;
        var exploredChars = 0;
        var node;
        
        while (node = walker.nextNode()) {
            var nodeLen = this.countChars(node.textContent);
            totalChars += nodeLen;
            
            if (nodeLen > 0) {
                var range = document.createRange();
                range.selectNodeContents(node);
                var rect = this.getRect(range);
                if ((vertical ? this.pageWidth - rect.right : rect.left) < 0) {
                    exploredChars += nodeLen;
                }
            }
        }
        
        return totalChars > 0 ? exploredChars / totalChars : 0;
    },
    
    registerSnapScroll(initialScroll) {
        if (window.snapScrollRegistered) {
            window.lastPageScroll = initialScroll;
            return;
        }
        window.snapScrollRegistered = true;
        window.lastPageScroll = initialScroll;
        
        document.body.addEventListener('scroll', function () {
            var vertical = window.hoshiReader.isVertical();
            var currentScroll = Math.abs(document.body.scrollLeft);
            var pageWidth = window.hoshiReader.pageWidth;
            var snappedScroll = Math.round(currentScroll / pageWidth) * pageWidth;
            if (Math.abs(currentScroll - snappedScroll) > 1) {
                document.body.scrollLeft = vertical ? -window.lastPageScroll : window.lastPageScroll;
            } else {
                window.lastPageScroll = snappedScroll;
            }
            window.hoshiReader.notifyPageChanged();
        }, { passive: true });
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
    
    notifyRestoreComplete() {
        this.notifyPageChanged();
        window.webkit?.messageHandlers?.restoreCompleted?.postMessage(null);
    },
    
    notifyPageChanged() {
        const page = Math.round(Math.abs(document.body.scrollLeft) / this.pageWidth) * this.pagesPerScreen;
        window.webkit?.messageHandlers?.pageChanged?.postMessage(page);
    },
    
    calculatePageStarts() {
        const { vertical, pageSize: screenSize, maxScroll } = this.getScrollContext();
        const pageSize = screenSize / this.pagesPerScreen;
        const pageCount = this.pagesPerScreen > 1
            ? Math.max(1, Math.floor(this.contentSize(vertical) / pageSize))
            : Math.round(maxScroll / pageSize) + 1;
        const starts = new Array(pageCount).fill(null);
        const walker = this.createWalker();
        const range = document.createRange();
        let totalChars = 0;
        let node;
        
        while (node = walker.nextNode()) {
            const nodeLen = this.countChars(node.textContent);
            if (!nodeLen) {
                continue;
            }
            const nodeStart = this.nodeStartOffsets.get(node);
            totalChars = nodeStart + nodeLen;
            range.selectNodeContents(node);
            for (const rect of range.getClientRects()) {
                if (!rect.width || !rect.height) {
                    continue;
                }
                const position = vertical ? screenSize - rect.right : rect.left;
                const page = Math.min(pageCount - 1, Math.max(0, Math.floor(position / pageSize)));
                if (starts[page] === null) {
                    let low = 0;
                    let high = node.textContent.length;
                    while (low < high) {
                        const mid = (low + high) >> 1;
                        range.setStart(node, mid);
                        range.setEnd(node, mid + 1);
                        const charRect = this.getRect(range);
                        const charPosition = vertical ? screenSize - charRect.right : charRect.left;
                        if (charPosition >= page * pageSize) {
                            high = mid;
                        } else {
                            low = mid + 1;
                        }
                    }
                    starts[page] = nodeStart + this.countChars(node.textContent.slice(0, low));
                }
            }
        }
        
        for (let page = pageCount - 1; page >= 0; page--) {
            starts[page] ??= starts[page + 1] ?? totalChars;
        }
        return starts;
    },
    
    contentSize(vertical) {
        var scrollSize = document.body.scrollWidth;
        if (!this.spacer) {
            return scrollSize;
        }
        var rect = this.spacer.getBoundingClientRect();
        var spacerStart = (vertical ? this.pageWidth - rect.right : rect.left) + Math.abs(document.body.scrollLeft);
        return Math.max(0, Math.min(scrollSize, spacerStart));
    },

    getScrollContext() {
        var vertical = this.isVertical();
        var scrollEl = document.body;
        var pageSize = this.pageWidth;
        var totalSize = this.contentSize(vertical);
        var maxScroll = Math.max(0, totalSize - pageSize);
        return { vertical, scrollEl, pageSize, maxScroll };
    },
    
    setScrollOffset(context, scroll) {
        var clampedScroll = Math.min(Math.max(0, scroll), context.maxScroll);
        context.scrollEl.scrollLeft = context.vertical ? -clampedScroll : clampedScroll;
        return clampedScroll;
    },
    
    alignToPage(context, anchor) {
        if (context.pageSize <= 0) {
            return 0;
        }
        var pageIndex = Math.floor(Math.max(0, anchor) / context.pageSize);
        return Math.min(Math.max(0, pageIndex * context.pageSize), context.maxScroll);
    },
    
    paginate(direction) {
        var vertical = this.isVertical();
        var pageSize = this.pageWidth;
        if (pageSize <= 0) return "limit";
        
        if (direction === "forward") {
            var totalSize = this.contentSize(vertical);
            var maxScroll = Math.max(0, totalSize - pageSize);
            var maxAlignedScroll = Math.round(maxScroll / pageSize) * pageSize;
            var currentScroll = Math.abs(document.body.scrollLeft);
            if ((currentScroll + pageSize) <= (maxAlignedScroll + 1)) {
                var targetScroll = Math.round((currentScroll + pageSize) / pageSize) * pageSize;
                window.lastPageScroll = targetScroll;
                document.body.scrollLeft = vertical ? -targetScroll : targetScroll;
                return "scrolled";
            }
            return "limit";
        } else {
            var currentScroll = Math.abs(document.body.scrollLeft);
            if (currentScroll > 0) {
                var targetScroll = Math.round((currentScroll - pageSize) / pageSize) * pageSize;
                window.lastPageScroll = targetScroll;
                document.body.scrollLeft = vertical ? -targetScroll : targetScroll;
                return "scrolled";
            }
            return "limit";
        }
    },
    
    scrollToRange(range) {
        if (window.hoshiContinuous?.sections.size) {
            return window.hoshiContinuous.reveal(range);
        }
        const context = this.getScrollContext();
        if (context.pageSize <= 0) {
            return false;
        }
        
        const rect = this.getRect(range);
        const currentScroll = Math.abs(context.scrollEl.scrollLeft);
        const anchor = (context.vertical ? this.pageWidth - (rect.left + rect.right) / 2 : (rect.left + rect.right) / 2) + currentScroll;
        const targetScroll = this.alignToPage(context, anchor);
        
        if (targetScroll === currentScroll) {
            return false;
        }
        
        window.lastPageScroll = targetScroll;
        this.setScrollOffset(context, targetScroll);
        requestAnimationFrame(() => {
            this.setScrollOffset(context, targetScroll);
        });
        
        return true;
    },
    
    collectSasayakiCueRanges(cues, root) {
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
        const walker = this.createWalker(root);
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
    
    applySasayakiCues(cues, root) {
        this.resetSasayakiCues(root);
        
        const cueRanges = this.collectSasayakiCueRanges(cues, root);
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
        
        this.refreshOffsets(root);
    },
    
    highlightSasayakiCue(cueId, reveal) {
        this.clearSasayakiCue();
        
        const wrappers = this.cueWrappers.get(cueId);
        if (!wrappers?.length) {
            return null;
        }
        
        this.activeCueId = cueId;
        wrappers.forEach(wrapper => wrapper.classList.add('hoshi-sasayaki-active'));
        
        if (reveal) {
            const range = document.createRange();
            range.selectNodeContents(wrappers[0]);
            if (this.scrollToRange(range)) {
                return this.currentProgress();
            }
        }
        
        return null;
    },
    
    clearSasayakiCue() {
        if (!this.activeCueId) {
            return;
        }
        
        const wrappers = this.cueWrappers.get(this.activeCueId) || [];
        wrappers.forEach(wrapper => wrapper.classList.remove('hoshi-sasayaki-active'));
        this.activeCueId = null;
    },
    
    scrollToSasayakiImage(index, root) {
        const el = (root || document).querySelectorAll('img, image')[index];
        if (!el || !(el.classList.contains('block-img') || el.namespaceURI === 'http://www.w3.org/2000/svg')) {
            return null;
        }
        
        const range = document.createRange();
        range.selectNode(el);
        const scrolled = this.scrollToRange(range);
        return { progress: scrolled ? this.currentProgress() : null };
    },
    
    resetSasayakiCues(root) {
        this.cueWrappers.forEach((wrappers, id) => {
            if (root && !root.contains(wrappers[0])) {
                return;
            }
            this.unwrap(wrappers);
            this.cueWrappers.delete(id);
            if (this.activeCueId === id) {
                this.activeCueId = null;
            }
        });
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
    
    async restoreProgress(progress) {
        await this.awaitFonts();
        var context = this.getScrollContext();

        if (context.pageSize <= 0) {
            this.registerSnapScroll(0);
            this.notifyRestoreComplete();
            return;
        }

        if (progress <= 0) {
            this.setScrollOffset(context, 0);
            this.registerSnapScroll(0);
            this.notifyRestoreComplete();
            return;
        }

        if (progress >= 0.99) {
            var lastPage = Math.round(context.maxScroll / context.pageSize) * context.pageSize;
            lastPage = Math.max(0, lastPage);
            this.setScrollOffset(context, lastPage);
            requestAnimationFrame(() => {
                this.setScrollOffset(context, lastPage);
                this.registerSnapScroll(lastPage);
                requestAnimationFrame(() => this.notifyRestoreComplete());
            });
            return;
        }

        var walker = this.createWalker();
        var totalChars = 0;
        var node;

        while (node = walker.nextNode()) {
            totalChars += this.countChars(node.textContent);
        }

        if (totalChars <= 0) {
            this.registerSnapScroll(0);
            this.notifyRestoreComplete();
            return;
        }

        var targetCharCount = Math.ceil(totalChars * progress);
        var runningSum = 0;
        var targetNode = null;

        walker = this.createWalker();
        while (node = walker.nextNode()) {
            runningSum += this.countChars(node.textContent);
            if (runningSum > targetCharCount) {
                targetNode = node;
                break;
            }
        }

        if (targetNode) {
            var range = document.createRange();
            range.setStart(targetNode, 0);
            range.setEnd(targetNode, 1);
            var rect = this.getRect(range);
            var anchor = (context.vertical ? this.pageWidth - (rect.left + rect.right) / 2 : (rect.left + rect.right) / 2) + Math.abs(context.scrollEl.scrollLeft);
            var targetScroll = this.alignToPage(context, anchor);

            this.setScrollOffset(context, targetScroll);
            requestAnimationFrame(() => {
                this.setScrollOffset(context, targetScroll);
                this.registerSnapScroll(targetScroll);
            });
        } else {
            this.registerSnapScroll(0);
        }

        requestAnimationFrame(() => {
            requestAnimationFrame(() => this.notifyRestoreComplete());
        });
    },
    
    async jumpToFragment(fragment) {
        await this.awaitFonts();
        var context = this.getScrollContext();
        var rawFragment = (fragment || '').trim();
        var target = rawFragment && (document.getElementById(rawFragment) || document.getElementsByName(rawFragment)[0]);
        
        if (context.pageSize <= 0 || !target) {
            this.registerSnapScroll(0);
            this.notifyRestoreComplete();
            return false;
        }
        
        var rect = this.getRect(target);
        var currentScroll = Math.abs(context.scrollEl.scrollLeft);
        var anchor = (context.vertical ? this.pageWidth - rect.right : rect.left) + currentScroll;
        var targetScroll = this.alignToPage(context, anchor);
        
        this.setScrollOffset(context, targetScroll);
        
        requestAnimationFrame(() => {
            this.setScrollOffset(context, targetScroll);
            this.registerSnapScroll(targetScroll);
            requestAnimationFrame(() => this.notifyRestoreComplete());
        });
        
        return true;
    }
};
