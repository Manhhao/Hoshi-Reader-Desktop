use unicode_normalization::UnicodeNormalization;

use crate::sasayaki::{
    Chapter, RE_CHARACTER, SasayakiMatch, SasayakiMatchData, SasayakiToken, Source, find_text,
};

const PROBE_LENGTH: usize = 12;
const PROBE_STRIDE: usize = 100;
const MAX_BLOCK: usize = 2000;
const MAX_SECONDS_PER_CHARACTER: f64 = 0.7;
const DURATION_OVERHEAD: f64 = 1.0;
const MAX_EDGE: usize = 12;
const MAX_INTERPOLATED: usize = 8;
const MAX_INTERPOLATED_GAP: f64 = 2.5;
const END_PADDING: f64 = 0.3;
const LEADING_GAP: f64 = 0.5;
const CHARACTER_SPAN: f64 = 0.15;
const START_LEAD: f64 = 0.2;
const EXACT_WINDOW: usize = 20;
const EXACT_SHARE: usize = 4;

#[derive(Clone, Copy)]
struct Timing {
    start: f64,
    end: f64,
    exact: bool,
}

struct Speech {
    characters: Vec<char>,
    timings: Vec<Timing>,
}

impl Speech {
    fn new(tokens: &[SasayakiToken]) -> Self {
        let mut characters = Vec::new();
        let mut timings = Vec::new();
        for token in tokens {
            for found in RE_CHARACTER.find_iter(&token.text) {
                let character = normalize(found.as_str().chars().next().unwrap());
                characters.push(character);
                timings.push(Timing {
                    start: token.start,
                    end: token.end,
                    exact: false,
                });
            }
        }
        Speech {
            characters,
            timings,
        }
    }
}

#[derive(Clone, Copy)]
struct Anchor {
    speech: usize,
    book: usize,
}

struct Span {
    from: f64,
    to: f64,
    lower: usize,
    upper: usize,
}

pub fn align(source: &Source, tokens: &[SasayakiToken]) -> SasayakiMatchData {
    let book: Vec<char> = source
        .text
        .iter()
        .map(|&character| normalize(character))
        .collect();
    let speech = Speech::new(tokens);
    let mut times = vec![None; book.len()];
    let anchors = anchors(&speech, &book, &source.chapters);
    for pair in anchors.windows(2) {
        let (anchor, next) = (pair[0], pair[1]);
        if next.speech <= anchor.speech || next.book <= anchor.book {
            continue;
        }

        let bounded = Anchor {
            speech: next.speech.min(anchor.speech + MAX_BLOCK),
            book: next.book.min(anchor.book + MAX_BLOCK),
        };
        assign(&speech, &book, anchor, bounded, &mut times);
        let tail = Anchor {
            speech: next.speech.saturating_sub(MAX_BLOCK).max(anchor.speech),
            book: next.book.saturating_sub(MAX_BLOCK).max(anchor.book),
        };
        if next.book < book.len() && (tail.speech > anchor.speech || tail.book > anchor.book) {
            assign(&speech, &book, tail, next, &mut times);
        }
    }

    cut(source, &times)
}

fn normalize(character: char) -> char {
    let mapped = std::iter::once(character)
        .nfkc()
        .collect::<String>()
        .to_lowercase();
    let scalar = mapped.chars().next().unwrap();
    if ('\u{30A1}'..='\u{30F6}').contains(&scalar) {
        return char::from_u32(scalar as u32 - 0x60).unwrap();
    }
    scalar
}

fn anchors(speech: &Speech, book: &[char], chapters: &[Chapter]) -> Vec<Anchor> {
    let mut anchors = vec![Anchor { speech: 0, book: 0 }];
    if let Some(chapter) = chapters
        .iter()
        .find(|chapter| chapter.length >= PROBE_LENGTH)
    {
        let probe = &book[chapter.start..chapter.start + PROBE_LENGTH];
        if find_text(book, probe, chapter.start + 1, book.len()).is_none()
            && let Some(found) = find_text(&speech.characters, probe, 0, speech.characters.len())
        {
            anchors[0] = Anchor {
                speech: found,
                book: chapter.start,
            };
        }
    }

    let mut cursor = 0;
    let mut hit = 0;
    if let Some(last) = speech.characters.len().checked_sub(PROBE_LENGTH) {
        for index in (0..=last).step_by(PROBE_STRIDE) {
            let probe = &speech.characters[index..index + PROBE_LENGTH];
            let limit = if cursor == 0 {
                book.len()
            } else {
                book.len()
                    .min(cursor + MAX_BLOCK + PROBE_LENGTH + index - hit)
            };
            let Some(found) = find_text(book, probe, cursor, limit) else {
                continue;
            };
            if find_text(book, probe, found + 1, limit).is_some() {
                continue;
            }

            cursor = found + PROBE_LENGTH;
            hit = index;
            let previous = anchors.last().unwrap();
            if index > previous.speech && found > previous.book {
                anchors.push(Anchor {
                    speech: index,
                    book: found,
                });
            }
        }
    }
    for chapter in chapters
        .iter()
        .filter(|chapter| chapter.length >= PROBE_LENGTH)
    {
        let probe = &book[chapter.start..chapter.start + PROBE_LENGTH];
        let after = anchors
            .iter()
            .position(|anchor| anchor.book > chapter.start)
            .unwrap_or(anchors.len());
        if anchors[after - 1].book == chapter.start {
            continue;
        }
        let low = anchors[after - 1].speech + 1;
        let high = anchors
            .get(after)
            .map_or(speech.characters.len(), |anchor| anchor.speech);
        let Some(found) = find_text(&speech.characters, probe, low, high) else {
            continue;
        };
        if find_text(&speech.characters, probe, found + 1, high).is_some() {
            continue;
        }
        anchors.insert(
            after,
            Anchor {
                speech: found,
                book: chapter.start,
            },
        );
    }
    anchors.push(Anchor {
        speech: speech.characters.len(),
        book: book.len(),
    });
    anchors
}

fn assign(
    speech: &Speech,
    book: &[char],
    anchor: Anchor,
    next: Anchor,
    times: &mut [Option<Timing>],
) {
    let spoken = &speech.characters[anchor.speech..next.speech];
    let written = &book[anchor.book..next.book];
    let (removed, inserted) = difference(spoken, written);

    let mut offset = 0;
    for (index, &removed) in removed.iter().enumerate() {
        if !removed {
            while offset < inserted.len() && inserted[offset] {
                offset += 1;
            }
        }

        let target = anchor.book + offset.min(written.len() - 1);
        if times[target].is_none() {
            times[target] = Some(Timing {
                exact: !removed,
                ..speech.timings[anchor.speech + index]
            });
        }

        let substitution = offset < inserted.len() && inserted[offset];
        if !removed || substitution {
            offset += 1;
        }
    }
}

fn difference(old: &[char], new: &[char]) -> (Vec<bool>, Vec<bool>) {
    let slot = |diagonal: isize| {
        if diagonal <= 0 {
            -diagonal as usize
        } else {
            diagonal as usize - 1
        }
    };
    let mut trace = Vec::new();
    let mut frontier = vec![0; 2];
    'search: for distance in 0..=old.len() + new.len() {
        trace.push(frontier);
        frontier = vec![0; distance + 1];
        let previous = trace.last().unwrap();
        let depth = distance as isize;
        for diagonal in (-depth..=depth).step_by(2) {
            let mut x = if diagonal == -depth
                || (diagonal != depth
                    && previous[slot(diagonal - 1)] < previous[slot(diagonal + 1)])
            {
                previous[slot(diagonal + 1)]
            } else {
                previous[slot(diagonal - 1)] + 1
            };
            let mut y = (x as isize - diagonal) as usize;
            while x < old.len() && y < new.len() && old[x] == new[y] {
                x += 1;
                y += 1;
            }
            frontier[slot(diagonal)] = x;
            if x >= old.len() && y >= new.len() {
                break 'search;
            }
        }
    }

    let mut removed = vec![false; old.len()];
    let mut inserted = vec![false; new.len()];
    let (mut x, mut y) = (old.len(), new.len());
    for distance in (1..trace.len()).rev() {
        let previous = &trace[distance];
        let diagonal = x as isize - y as isize;
        let depth = distance as isize;
        let preceding = if diagonal == -depth
            || (diagonal != depth && previous[slot(diagonal - 1)] < previous[slot(diagonal + 1)])
        {
            diagonal + 1
        } else {
            diagonal - 1
        };
        let from_x = previous[slot(preceding)];
        let from_y = (from_x as isize - preceding) as usize;
        while x > from_x && y > from_y {
            x -= 1;
            y -= 1;
        }
        if y != from_y {
            inserted[from_y] = true;
        } else {
            removed[from_x] = true;
        }
        (x, y) = (from_x, from_y);
    }
    (removed, inserted)
}

fn cut(source: &Source, times: &[Option<Timing>]) -> SasayakiMatchData {
    let mut matches: Vec<SasayakiMatch> = Vec::new();
    let mut unmatched = 0;
    for chapter in &source.chapters {
        let mut start = chapter.start;
        for index in chapter.start..chapter.end() {
            if !is_boundary(source, chapter, start, index) {
                continue;
            }

            let previous = matches.last().map(|previous| previous.end_time);
            if let Some(span) = time(start, index, chapter, times, previous) {
                matches.push(SasayakiMatch {
                    id: format!("{}-{}", chapter.chapter_index, span.lower - chapter.start),
                    start_time: span.from,
                    end_time: span.to,
                    text: source.text[span.lower..=span.upper].iter().collect(),
                    chapter_index: chapter.chapter_index,
                    start: span.lower - chapter.start,
                    length: span.upper - span.lower + 1,
                });
            } else {
                unmatched += 1;
            }
            start = index + 1;
        }
    }

    matches.sort_by(|a, b| a.start_time.total_cmp(&b.start_time));
    for index in 0..matches.len() {
        let limit = matches
            .get(index + 1)
            .map_or(f64::INFINITY, |next| next.start_time);
        let end = matches[index].end_time;
        matches[index].end_time = end.max((end + END_PADDING).min(limit));
    }

    SasayakiMatchData {
        matches,
        unmatched,
        images: source.images.clone(),
    }
}

fn is_boundary(source: &Source, chapter: &Chapter, start: usize, index: usize) -> bool {
    if source.sentence_ends[index] || index == chapter.end() - 1 {
        return true;
    }

    source.segment_ends[index]
        && index > start
        && index + 2 < chapter.end()
        && !source.sentence_ends[index + 1]
}

fn time(
    start: usize,
    index: usize,
    chapter: &Chapter,
    times: &[Option<Timing>],
    previous: Option<f64>,
) -> Option<Span> {
    let low = start.saturating_sub(EXACT_WINDOW).max(chapter.start);
    let high = (index + EXACT_WINDOW).min(chapter.end() - 1);
    let exact = (low..=high)
        .filter(|&i| times[i].is_some_and(|timing| timing.exact))
        .count();
    if exact * EXACT_SHARE < high - low + 1 {
        return None;
    }
    let timed: Vec<usize> = (start..=index).filter(|&i| times[i].is_some()).collect();
    let (Some(&first), Some(&last)) = (timed.first(), timed.last()) else {
        return interpolate(start, index, chapter, times, previous);
    };

    let first_timing = times[first].unwrap();
    let to = times[last].unwrap().end;
    let lower = if first - start <= MAX_EDGE {
        start
    } else {
        first
    };
    let upper = if index - last <= MAX_EDGE {
        index
    } else {
        last
    };
    let mut from = first_timing.start;
    if timed.len() >= 2 && times[timed[1]].unwrap().start - first_timing.end > LEADING_GAP {
        from = times[timed[1]].unwrap().start;
    } else {
        from = from.max(first_timing.end - CHARACTER_SPAN);
    }
    from -= START_LEAD;

    if !within_budget(from, to, upper - lower + 1) {
        return trim(start, index, &timed, times)
            .or_else(|| interpolate(start, index, chapter, times, previous));
    }

    Some(Span {
        from,
        to,
        lower,
        upper,
    })
}

fn trim(start: usize, index: usize, timed: &[usize], times: &[Option<Timing>]) -> Option<Span> {
    let at = |position: usize| times[timed[position]].unwrap();
    let (mut first, mut last) = (0, timed.len() - 1);
    while first < last && at(first + 1).start - at(first).end > LEADING_GAP {
        first += 1;
    }
    while last > first && at(last).start - at(last - 1).end > LEADING_GAP {
        last -= 1;
    }

    let lower = if timed[first] - start <= MAX_EDGE {
        start
    } else {
        timed[first]
    };
    let upper = if index - timed[last] <= MAX_EDGE {
        index
    } else {
        timed[last]
    };
    let from = at(first).start.max(at(first).end - CHARACTER_SPAN) - START_LEAD;
    let to = at(last).end;
    within_budget(from, to, upper - lower + 1).then_some(Span {
        from,
        to,
        lower,
        upper,
    })
}

fn interpolate(
    start: usize,
    index: usize,
    chapter: &Chapter,
    times: &[Option<Timing>],
    previous: Option<f64>,
) -> Option<Span> {
    let length = index - start + 1;
    if length > MAX_INTERPOLATED || start <= chapter.start || index + 1 >= chapter.end() {
        return None;
    }
    let previous_end = times[start - 1]?.end;
    let next_start = times[index + 1]?.start;
    if next_start <= previous_end || next_start - previous_end > MAX_INTERPOLATED_GAP {
        return None;
    }

    let mut from = (previous_end + END_PADDING).min(next_start);
    if let Some(previous) = previous {
        from = from.max(previous);
    }

    if !within_budget(from, next_start, length) {
        return None;
    }

    Some(Span {
        from,
        to: next_start,
        lower: start,
        upper: index,
    })
}

fn within_budget(from: f64, to: f64, span: usize) -> bool {
    to > from && (to - from) <= DURATION_OVERHEAD + MAX_SECONDS_PER_CHARACTER * span as f64
}
