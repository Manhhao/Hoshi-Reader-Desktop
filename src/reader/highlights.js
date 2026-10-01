window.hoshiHighlights = {
    highlights: new Map(),
    searchHighlight: null,
    
    rootOf(node) {
        return node.parentElement?.closest('hoshi-section') || document.body;
    },
    
    createHighlight(color, id) {
        const selection = window.getSelection();
        const range = selection.getRangeAt(0);
        const root = this.rootOf(range.startContainer);
        
        const startPrefix = range.startContainer.textContent.substring(0, range.startOffset);
        const endPrefix = range.endContainer.textContent.substring(0, range.endOffset);
        
        const start = window.hoshiReader.nodeStartOffsets.get(range.startContainer) + window.hoshiReader.countChars(startPrefix);
        const rawStart = window.hoshiReader.nodeStartRawOffsets.get(range.startContainer) + window.hoshiReader.countRawChars(startPrefix);
        const rawEnd = window.hoshiReader.nodeStartRawOffsets.get(range.endContainer) + window.hoshiReader.countRawChars(endPrefix);
        if (rawEnd <= rawStart) {
            return null;
        }
        
        const existing = this.findHighlight(rawStart, rawEnd - rawStart, root);
        if (existing) {
            selection.removeAllRanges();
            return this.updateHighlight(existing, color);
        }
        
        const fragment = range.cloneContents();
        fragment.querySelectorAll('rt, rp').forEach(el => el.remove());
        const text = fragment.textContent;
        
        const textFurigana = this.collectSegments(rawStart, Array.from(text).length, false, root).map(segment => {
            const t = segment.node.textContent.slice(segment.start, segment.end);
            let rt = segment.node.parentElement.nextElementSibling;
            while (rt?.matches('rp')) {
                rt = rt.nextElementSibling;
            }
            return rt?.matches('rt') && rt.textContent ? `${t}(${rt.textContent})` : t;
        }).join('');
        
        selection.removeAllRanges();
        
        this.wrapHighlight({ id, color, offset: rawStart, text }, root);
        window.hoshiReader.refreshOffsets(root);
        
        requestAnimationFrame(() => {
            document.body.style.transform = 'translateZ(0)';
            requestAnimationFrame(() => {
                document.body.style.transform = '';
            });
        });
        
        return { start, offset: rawStart, text, textFurigana: textFurigana !== text ? textFurigana : null, spine: root.hoshiSection?.i ?? null };
    },
    
    findHighlight(offset, length, root) {
        for (const [id, entry] of this.highlights) {
            if (entry.offset === offset && entry.length === length && entry.root === root) {
                return id;
            }
        }
        return null;
    },
    
    updateHighlight(id, color) {
        const entry = this.highlights.get(id);
        if (entry.color === color) {
            this.removeHighlight(id);
        } else {
            entry.color = color;
            entry.wrappers.forEach(wrapper => {
                wrapper.className = `hoshi-highlight hoshi-highlight-${color}`;
            });
        }
        return { id };
    },
    
    collectSegments(offset, length, filtered, root) {
        const end = offset + length;
        const segments = [];
        let cursor = 0;
        let segment = null;
        
        const flushSegment = () => {
            if (!segment) {
                return;
            }
            
            segments.push(segment);
            segment = null;
        };
        
        let node;
        const walker = window.hoshiReader.createWalker(root);
        while (cursor < end && (node = walker.nextNode())) {
            const text = node.textContent;
            let i = 0;
            while (i < text.length && cursor < end) {
                const char = String.fromCodePoint(text.codePointAt(i));
                const next = i + char.length;
                const matchable = !filtered || window.hoshiReader.isMatchableChar(char);
                const inside = matchable ? cursor >= offset : cursor > offset;
                
                if (inside) {
                    if (!segment || segment.node !== node) {
                        flushSegment();
                        segment = { node, start: i, end: next };
                    } else {
                        segment.end = next;
                    }
                }
                
                if (matchable) {
                    cursor += 1;
                }
                i = next;
            }
            flushSegment();
        }
        
        return segments;
    },
    
    wrapHighlight(highlight, root) {
        const { id, color, offset, text } = highlight;
        const length = window.hoshiReader.countRawChars(text);
        const segments = this.collectSegments(offset, length, false, root);
        if (!segments.length) {
            return;
        }
        
        const range = document.createRange();
        const wrappers = [];
        for (let i = segments.length - 1; i >= 0; i--) {
            const s = segments[i];
            range.setStart(s.node, s.start);
            range.setEnd(s.node, s.end);
            
            const wrapper = document.createElement('span');
            wrapper.className = `hoshi-highlight hoshi-highlight-${color}`;
            wrapper.appendChild(range.extractContents());
            range.insertNode(wrapper);
            
            wrappers.push(wrapper);
        }
        wrappers.reverse();
        this.highlights.set(id, { color, offset, length, wrappers, root: root || document.body });
    },
    
    applyHighlights(highlights, root) {
        for (const h of highlights) {
            this.wrapHighlight(h, root);
        }
        window.hoshiReader.refreshOffsets(root);
    },
    
    drop(root) {
        for (const [id, entry] of this.highlights) {
            if (entry.root === root) {
                this.highlights.delete(id);
            }
        }
    },
    
    removeHighlight(id) {
        const entry = this.highlights.get(id);
        if (!entry) {
            return;
        }
        
        window.hoshiReader.unwrap(entry.wrappers);
        this.highlights.delete(id);
        window.hoshiReader.refreshOffsets(entry.root);
        
        requestAnimationFrame(() => {
            document.body.style.transform = 'translateZ(0)';
            requestAnimationFrame(() => {
                document.body.style.transform = '';
            });
        });
    },
    
    showSearchHighlight(offset, length, root) {
        if (!this.searchHighlight) {
            this.searchHighlight = new Highlight();
            CSS.highlights.set('hoshi-search', this.searchHighlight);
        }
        this.searchHighlight.clear();
        
        for (const segment of this.collectSegments(offset, length, true, root)) {
            const range = document.createRange();
            range.setStart(segment.node, segment.start);
            range.setEnd(segment.node, segment.end);
            this.searchHighlight.add(range);
        }
    },
    
    clearSearchHighlight() {
        this.searchHighlight?.clear();
    }
};
